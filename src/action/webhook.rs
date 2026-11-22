use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::action::{ActionCours, ContexteAction, template_applique, variables};
use crate::config::Config;
use crate::error::{Error, Result};

#[derive(Debug)]
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
    if !hote_valide(host) {
        return Err(Error::new("webhook : hote invalide"));
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
    if !chemin_valide(&chemin) {
        return Err(Error::new("webhook : chemin invalide"));
    }
    Ok(Cible {
        host: host.to_string(),
        port,
        chemin,
    })
}

/// Lettres, chiffres, points et tirets seulement : un hote avec espaces
/// ou caracteres de controle injecterait des en-tetes HTTP.
fn hote_valide(host: &str) -> bool {
    !host.starts_with(['.', '-'])
        && !host.ends_with('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// Caracteres ASCII visibles sans espace ni controle : le chemin part
/// verbatim dans la ligne de requete.
fn chemin_valide(chemin: &str) -> bool {
    chemin.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Adresse interne (loopback, privee, lien local, metadonnees, test,
/// multicast) : destination classique de SSRF depuis un daemon.
pub fn interne(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            v.is_loopback()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || v.is_private()
                || v.is_link_local()
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 2)
                || (o[0] == 198 && o[1] == 51 && o[2] == 100)
                || (o[0] == 203 && o[1] == 0 && o[2] == 113)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0xdb8)
        }
    }
}

pub fn json_echappe(s: &str) -> String {
    // RFC 8259 : guillemets, antislash et tous les caractères de contrôle
    // U+0000 à U+001F doivent être échappés.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
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
    let local = ac.act.local == Some(true);
    // La resolution fait foi : la connexion part sur une adresse deja
    // verifiee, pas sur une re-resolution qui pourrait rebondir ailleurs.
    let adresses: Vec<SocketAddr> = (cible.host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| Error::new(format!("webhook : resolution impossible : {e}")))?
        .collect();
    if adresses.is_empty() {
        return Err(Error::new("webhook : resolution impossible"));
    }
    if !local {
        for a in &adresses {
            if interne(&a.ip()) {
                return Err(Error::new(
                    "webhook : destination interne refusee (local: oui pour autoriser)",
                ));
            }
        }
    }
    let body = corps(ac, ctx);
    let req = requete(&cible, &body);
    let delai = delai_webhook(ac, cfg);
    let mut flux = None;
    for a in &adresses {
        if let Ok(f) = TcpStream::connect_timeout(a, delai) {
            flux = Some(f);
            break;
        }
    }
    let mut flux = flux.ok_or_else(|| Error::new("webhook : connexion impossible"))?;
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
    use std::net::IpAddr;

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
        for url in [
            "https://exemple.invalid/hook",
            "http://",
            "http://127.0.0.1:/hook",
            "http://h:1234567890123456/x",
        ] {
            assert!(cible_depuis_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn hote_et_chemin_injectables_refuses() {
        for url in [
            "http://hote exemple/x", // espace : injection d'en-tete
            "http://hote\n.bad/x",   // retour ligne : injection d'en-tete
            "http://[::1]:9999/x",   // crochets hors grammaire host
            "http://hote.bad/ chemin",
            "http://hote.bad/x\r\nHost: autre",
        ] {
            let e = cible_depuis_url(url).expect_err(url);
            let m = e.to_string();
            assert!(
                m.contains("hote invalide") || m.contains("chemin invalide"),
                "{url} : {m}"
            );
        }
        assert!(cible_depuis_url("http://mon-serveur.domaine/a?b=1&c=d").is_ok());
        assert!(cible_depuis_url("http://hote.bad").is_ok());
    }

    #[test]
    fn adresses_internes() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.5.5",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "192.0.2.5",
            "198.51.100.5",
            "203.0.113.5",
            "198.18.0.5",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "2001:db8::1",
        ] {
            let ip: IpAddr = ip.parse().expect("ip");
            assert!(interne(&ip), "{ip} doit etre interne");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "100.128.0.1", "172.32.0.1", "2606:4700::1111"] {
            let ip: IpAddr = ip.parse().expect("ip");
            assert!(!interne(&ip), "{ip} doit etre publique");
        }
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
    fn json_echappe_caracteres_de_controle() {
        // RFC 8259 : U+0000 à U+001F échappés, sinon le JSON du corps est invalide.
        let brut = "ligne\n tab\t guillement \" citation \\ antislash \u{7} fin";
        let echappe = json_echappe(brut);
        assert_eq!(
            echappe,
            "ligne\\n tab\\t guillement \\\" citation \\\\ antislash \\u0007 fin"
        );
        // le résultat doit reformer un JSON valide
        let corps = format!("{{\"message\":\"{echappe}\"}}");
        assert!(corps.contains("\\n"), "{corps}");
        assert!(
            !corps.contains('\n'),
            "aucun saut de ligne brut dans le corps"
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

    #[test]
    fn destination_interne_refusee_par_defaut() {
        let ctx = ContexteAction {
            dry_run: false,
            hostname: "h".to_string(),
        };
        let act = ReglePret::nouveau();
        let ac = ActionCours {
            act: &act.act,
            regle: &act.regle,
            valeur: 50.0,
        };
        let cfg = Config::default();
        let e = envoit(&ac, &cfg, &ctx).expect_err("interne refusee");
        assert!(
            e.to_string().contains("destination interne refusee"),
            "{e}"
        );
    }

    #[test]
    fn destination_interne_autorisee_par_local() {
        let ctx = ContexteAction {
            dry_run: false,
            hostname: "h".to_string(),
        };
        let mut act = ReglePret::nouveau();
        act.act.url = Some("http://127.0.0.1:1/hook".to_string());
        act.act.local = Some(true);
        let ac = ActionCours {
            act: &act.act,
            regle: &act.regle,
            valeur: 50.0,
        };
        let cfg = Config::default();
        let e = envoit(&ac, &cfg, &ctx).expect_err("rien n'ecoute sur le port 1");
        assert!(
            e.to_string().contains("connexion impossible"),
            "{e}"
        );
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
                url: Some("http://127.0.0.1:9999/hook".to_string()),
                command: None,
                message: Some("alerte {{.Value}} \"prioritaire\" {{.Hostname}}".to_string()),
                niveau_log: None,
                local: None,
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
