use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::action::{ActionCours, ContexteAction, template_applique, variables};
use crate::config::Config;
use crate::error::{Error, Result};

pub struct Cible {
    pub host: String,
    pub port: String,
    pub chemin: String,
}

pub fn cible_depuis_url(url: &str) -> Result<Cible> {
    let reste = url
        .strip_prefix("http://")
        .ok_or_else(|| Error::new("webhook : https non supporte"))?;
    let fin = reste.find('/').unwrap_or(reste.len());
    let colon = reste[..fin].find(':');
    let (host, port) = match colon {
        Some(c) => (&reste[..c], Some(&reste[c + 1..fin])),
        None => (&reste[..fin], None),
    };
    if host.is_empty() {
        return Err(Error::new("webhook : hote absent"));
    }
    let port = match port {
        None => "80".to_string(),
        Some(p) if p.is_empty() || p.len() >= 16 => {
            return Err(Error::new("webhook : port invalide"));
        }
        Some(p) => p.to_string(),
    };
    let chemin = if fin < reste.len() {
        reste[fin..].to_string()
    } else {
        String::new()
    };
    Ok(Cible {
        host: host.to_string(),
        port,
        chemin,
    })
}

pub fn json_echappe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

pub fn corps(ac: &ActionCours, ctx: &ContexteAction) -> String {
    let v = variables(ac.regle, ac.valeur, &ctx.hostname);
    let nom = json_echappe(&v.nom);
    let hote = json_echappe(&v.hostname);
    let message = ac
        .act
        .message
        .as_deref()
        .map(|m| json_echappe(&template_applique(m, &v)))
        .unwrap_or_default();
    format!(
        "{{\"rule\":\"{nom}\",\"severity\":\"{}\",\"value\":{},\"threshold\":{},\
         \"hostname\":\"{hote}\",\"timestamp\":\"{}\",\"message\":\"{message}\"}}",
        v.severite, v.valeur, v.seuil, v.horodatage
    )
}

pub fn requete(cible: &Cible, corps: &str) -> String {
    format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        cible.chemin,
        cible.host,
        corps.len(),
        corps
    )
}

fn delai_webhook(ac: &ActionCours, cfg: &Config) -> Duration {
    let ms = match ac.act.timeout {
        Some(t) if t >= 0 => t,
        _ => cfg.webhook_timeout,
    };
    Duration::from_millis(u64::try_from(ms).unwrap_or(u64::MAX))
}

pub fn envoit(ac: &ActionCours, cfg: &Config, ctx: &ContexteAction) -> Result<()> {
    let Some(url) = ac.act.url.as_deref() else {
        return Err(Error::new("webhook : url absente"));
    };
    let cible = cible_depuis_url(url)?;
    let port: u16 = cible
        .port
        .parse()
        .map_err(|_| Error::new("webhook : port invalide"))?;
    let body = corps(ac, ctx);
    let req = requete(&cible, &body);
    let delai = delai_webhook(ac, cfg);
    let mut flux = TcpStream::connect((cible.host.as_str(), port))
        .map_err(|e| Error::new(format!("webhook : connexion impossible : {e}")))?;
    flux.set_write_timeout(Some(delai))
        .map_err(|e| Error::new(format!("webhook : {e}")))?;
    flux.set_read_timeout(Some(delai))
        .map_err(|e| Error::new(format!("webhook : {e}")))?;
    flux.write_all(req.as_bytes())
        .map_err(|e| Error::new(format!("webhook : envoi interrompu : {e}")))?;
    let mut reponse = Vec::new();
    let mut lecture = (&flux).take(2047);
    let _ = lecture.read_to_end(&mut reponse);
    if accepte(&reponse) {
        Ok(())
    } else {
        Err(Error::new("webhook : reponse refusee"))
    }
}

pub fn accepte(reponse: &[u8]) -> bool {
    let texte = String::from_utf8_lossy(reponse);
    texte.contains(" 200 ") || texte.contains(" 201 ") || texte.contains(" 204 ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cible_standard() {
        let c = cible_depuis_url("http://exemple.invalid/hook").expect("valide");
        assert_eq!(c.host, "exemple.invalid");
        assert_eq!(c.port, "80");
        assert_eq!(c.chemin, "/hook");
    }

    #[test]
    fn cible_avec_port_et_chemin_profond() {
        let c = cible_depuis_url("http://127.0.0.1:9999/a/b").expect("valide");
        assert_eq!(c.port, "9999");
        assert_eq!(c.chemin, "/a/b");
        let c = cible_depuis_url("http://hote:8080").expect("sans chemin");
        assert_eq!(c.chemin, "");
    }

    #[test]
    fn cible_refus() {
        assert!(cible_depuis_url("https://exemple.invalid/hook").is_err());
        assert!(cible_depuis_url("http://").is_err());
        assert!(cible_depuis_url("http://127.0.0.1:/hook").is_err());
        assert!(cible_depuis_url("http://h:1234567890123456/x").is_err());
    }

    #[test]
    fn corps_json() {
        let regle = ReglePret::nouveau();
        let ctx = ContexteAction {
            dry_run: false,
            hostname: "hote-local".to_string(),
        };
        let ac = ActionCours {
            act: &regle.act,
            regle: &regle.regle,
            valeur: 80.0,
        };
        let body = corps(&ac, &ctx);
        assert!(body.starts_with("{\"rule\":\"web\""));
        assert!(body.contains("\"severity\":\"warning\""));
        assert!(body.contains("\"value\":80"));
        assert!(body.contains("\"threshold\":50"));
        assert!(body.contains("\"hostname\":\"hote-local\""));
        assert!(body.contains("\"timestamp\":\"2"));
        assert!(
            body.contains("\"message\":\"alerte 80 \\\"prioritaire\\\" hote-local\""),
            "{body}"
        );
    }

    #[test]
    fn requete_http() {
        let c = cible_depuis_url("http://h.example/x").expect("valide");
        let req = requete(&c, "{}");
        assert!(req.starts_with("POST /x HTTP/1.1\r\nHost: h.example\r\n"));
        assert!(req.contains("Content-Type: application/json\r\n"));
        assert!(req.contains("Content-Length: 2\r\n"));
        assert!(req.contains("Connection: close\r\n\r\n{}"));
    }

    #[test]
    fn codes_acceptes() {
        let rep = b"HTTP/1.1 200 OK\r\n\r\nok";
        assert!(accepte(rep));
        let rep = b"HTTP/1.1 201 Created\r\n\r\n";
        assert!(accepte(rep));
        let rep = b"HTTP/1.1 204 No Content\r\n\r\n";
        assert!(accepte(rep));
        let rep = b"HTTP/1.1 500 KO\r\n\r\n";
        assert!(!accepte(rep));
    }

    struct ReglePret {
        regle: crate::config::Regle,
        act: crate::config::Action,
    }

    impl ReglePret {
        fn nouveau() -> Self {
            let act = crate::config::Action {
                kind: crate::config::TypeAction::Webhook,
                timeout: None,
                url: None,
                command: None,
                message: Some("alerte {{.Value}} \"prioritaire\" {{.Hostname}}".to_string()),
                niveau_log: None,
            };
            let regle = crate::config::Regle {
                nom: "web".to_string(),
                severite: crate::config::Severite::Warning,
                condition: crate::config::Condition {
                    metrique: None,
                    montage: None,
                    operateur: None,
                    valeur: 50.0,
                },
                pour: 0,
                cooldown: 0,
                actions: Vec::new(),
            };
            Self { regle, act }
        }
    }
}
