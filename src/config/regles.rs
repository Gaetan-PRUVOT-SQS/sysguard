use crate::error::{Error, Result};
use crate::yaml::Node;

use super::{Action, Condition, Config, Regle, Severite, actions, condition, scalaire};

pub fn parse(noeud: &Node, cfg: &mut Config) -> Result<()> {
    let Node::List(items) = noeud else {
        return Err(Error::new("rules doit etre une liste"));
    };
    cfg.regles = Vec::with_capacity(items.len());
    for item in items {
        cfg.regles.push(parse_regle(item)?);
    }
    valider_doublons(&cfg.regles)?;
    Ok(())
}

fn parse_regle(item: &Node) -> Result<Regle> {
    let mut nom: Option<String> = None;
    let mut severite = Severite::Warning;
    let mut cond = Condition::default();
    let mut pour = 0;
    let mut cooldown = 0;
    let mut actions_liste: Option<Vec<Action>> = None;

    let entries = match item {
        Node::Map(entries) => entries.as_slice(),
        _ => &[],
    };
    for (cle, noeud) in entries {
        match cle.as_str() {
            "name" => nom = Some(champ_nom(noeud)?),
            "severity" => severite = champ_severite(noeud)?,
            "for" => pour = super::duree_libre(cle, scalaire(noeud, cle)?)?,
            "cooldown" => cooldown = super::duree_libre(cle, scalaire(noeud, cle)?)?,
            "condition" => condition::parse(noeud, &mut cond)?,
            "actions" => actions::parse(noeud, &mut actions_liste)?,
            _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
        }
    }
    let Some(nom) = nom else {
        return Err(Error::new("regle : nom requis"));
    };
    let Some(actions) = actions_liste else {
        return Err(Error::new("regle : au moins une action requise"));
    };
    Ok(Regle {
        nom,
        severite,
        condition: cond,
        pour,
        cooldown,
        actions,
    })
}

fn champ_nom(noeud: &Node) -> Result<String> {
    let valeur = match noeud {
        Node::Scalar(s) if !s.is_empty() => s,
        _ => return Err(Error::new("name doit etre une chaine non vide")),
    };
    Ok(valeur.clone())
}

fn champ_severite(noeud: &Node) -> Result<Severite> {
    match crate::yaml::valeur_texte(noeud) {
        "info" => Ok(Severite::Info),
        "warning" => Ok(Severite::Warning),
        "critical" => Ok(Severite::Critical),
        autre => Err(Error::new(format!("severite inconnue : {autre}"))),
    }
}

fn valider_doublons(regles: &[Regle]) -> Result<()> {
    for (i, regle) in regles.iter().enumerate() {
        for autre in &regles[..i] {
            if autre.nom == regle.nom {
                return Err(Error::new(format!(
                    "regle {} : nom duplique : {}",
                    i + 1,
                    regle.nom
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TypeAction;

    #[test]
    fn severites_connues() {
        let cfg = crate::config::parse(
            "rules:\n  - name: a\n    severity: info\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 1\n    actions:\n      - type: log\n",
        )
        .expect("valide");
        assert_eq!(cfg.regles[0].severite, Severite::Info);
    }

    #[test]
    fn doublon_cite_numero() {
        let yaml = "rules:\n  - name: a\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 1\n    actions:\n      - type: log\n  - name: a\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 1\n    actions:\n      - type: log\n";
        let e = crate::config::parse(yaml).expect_err("doublon");
        let message = e.to_string();
        assert!(message.contains("regle 2 : nom duplique : a"), "{message}");
    }

    #[test]
    fn type_action_log_par_defaut_existe() {
        assert_ne!(TypeAction::Log, TypeAction::Exec);
    }
}
