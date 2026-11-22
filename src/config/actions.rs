use crate::error::{Error, Result};
use crate::log::level_from_text;
use crate::yaml::Node;

use super::{Action, TypeAction, scalaire};

pub fn parse(noeud: &Node, dest: &mut Option<Vec<Action>>) -> Result<()> {
    let Node::List(items) = noeud else {
        return Err(Error::new("actions doit etre une liste"));
    };
    let mut vec = Vec::with_capacity(items.len());
    for item in items {
        vec.push(parse_action(item)?);
    }
    verifie(&vec)?;
    *dest = Some(vec);
    Ok(())
}

fn parse_action(item: &Node) -> Result<Action> {
    let mut act = Action {
        kind: TypeAction::Log,
        timeout: None,
        url: None,
        command: None,
        message: None,
        niveau_log: None,
    };
    let Node::Map(entries) = item else {
        return Err(Error::new("action : carte attendue"));
    };
    for (cle, noeud) in entries {
        champ(cle, noeud, &mut act)?;
    }
    Ok(act)
}

fn champ(cle: &str, noeud: &Node, act: &mut Action) -> Result<()> {
    let valeur = scalaire(noeud, cle)?;
    match cle {
        "type" => act.kind = type_action(valeur)?,
        "url" => act.url = Some(valeur.to_string()),
        "command" => act.command = Some(valeur.to_string()),
        "message" => act.message = Some(valeur.to_string()),
        "level" => {
            act.niveau_log = Some(
                level_from_text(valeur)
                    .map_err(|_| Error::new(format!("niveau inconnu : {valeur}")))?,
            );
        }
        "timeout" => act.timeout = Some(duree_timeout(valeur)?),
        _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
    }
    Ok(())
}

fn type_action(valeur: &str) -> Result<TypeAction> {
    match valeur {
        "log" => Ok(TypeAction::Log),
        "webhook" => Ok(TypeAction::Webhook),
        "exec" => Ok(TypeAction::Exec),
        _ => Err(Error::new(format!("type d'action inconnu : {valeur}"))),
    }
}

fn duree_timeout(valeur: &str) -> Result<i64> {
    match crate::duration::parse_duration(valeur) {
        Ok(d) if d >= 0 => Ok(d),
        _ => Err(Error::new(format!("timeout : duree invalide : {valeur}"))),
    }
}

fn verifie(actions: &[Action]) -> Result<()> {
    if actions.is_empty() {
        return Err(Error::new("au moins une action requise"));
    }
    for (i, act) in actions.iter().enumerate() {
        verifie_une(act, i + 1)?;
    }
    Ok(())
}

fn verifie_une(act: &Action, numero: usize) -> Result<()> {
    match act.kind {
        TypeAction::Log => {
            if act.timeout.is_some() || act.url.is_some() || act.command.is_some() {
                return Err(Error::new(format!(
                    "action {numero} : champ interdit pour log"
                )));
            }
        }
        TypeAction::Webhook => {
            if act.command.is_some() {
                return Err(Error::new(format!(
                    "action {numero} : command interdit pour webhook"
                )));
            }
            verifie_url(act, numero)?;
        }
        TypeAction::Exec => {
            let Some(command) = &act.command else {
                return Err(Error::new(format!("action {numero} : command requis")));
            };
            if !command.starts_with('/') {
                return Err(Error::new(format!(
                    "action {numero} : command doit etre absolu"
                )));
            }
            if act.url.is_some() {
                return Err(Error::new(format!(
                    "action {numero} : url interdite pour exec"
                )));
            }
        }
    }
    Ok(())
}

fn verifie_url(act: &Action, numero: usize) -> Result<()> {
    let Some(url) = &act.url else {
        return Err(Error::new(format!("action {numero} : url requise")));
    };
    let suite = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(suite) = suite else {
        return Err(Error::new(format!(
            "action {numero} : url invalide : {url}"
        )));
    };
    if suite.is_empty() || suite.starts_with('/') {
        return Err(Error::new(format!(
            "action {numero} : url invalide : {url}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::valeur_texte;

    #[test]
    fn url_valide_aux_bornes() {
        let act = Action {
            kind: TypeAction::Webhook,
            timeout: None,
            url: Some("http://ntfy.sh/mon-topic".to_string()),
            command: None,
            message: None,
            niveau_log: None,
        };
        assert!(verifie_une(&act, 1).is_ok());
        let act_https = Action {
            url: Some("https://ntfy.sh".to_string()),
            ..act
        };
        assert!(verifie_une(&act_https, 1).is_ok());
    }

    #[test]
    fn url_invalide() {
        for url in [
            "ftp://x",
            "http://",
            "https://",
            "http:///host",
            "sans-scheme",
        ] {
            let act = Action {
                kind: TypeAction::Webhook,
                timeout: None,
                url: Some(url.to_string()),
                command: None,
                message: None,
                niveau_log: None,
            };
            let e = verifie_une(&act, 1).expect_err(url);
            assert!(e.to_string().contains("url invalide"), "{e}");
        }
        let act = Action {
            kind: TypeAction::Webhook,
            timeout: None,
            url: None,
            command: None,
            message: Some("sans url".to_string()),
            niveau_log: None,
        };
        let e = verifie_une(&act, 1).expect_err("requise");
        assert!(e.to_string().contains("url requise"));
    }

    #[test]
    fn liste_vide_refusee() {
        let e = verifie(&[]).expect_err("vide");
        assert!(e.to_string().contains("au moins une action requise"));
    }

    #[test]
    fn valeur_texte_sur_carte_est_vide() {
        let noeud = Node::Map(Vec::new());
        assert_eq!(valeur_texte(&noeud), "");
    }
}
