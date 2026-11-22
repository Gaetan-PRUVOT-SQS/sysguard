use crate::error::{Error, Result};
use crate::log::level_from_text;
use crate::yaml::Node;

use super::{Config, scalaire};

pub fn parse(carte: &Node, cfg: &mut Config) -> Result<()> {
    let Node::Map(entries) = carte else {
        return Err(Error::new("global : carte attendue"));
    };
    for (cle, noeud) in entries {
        champ(cle, noeud, cfg)?;
    }
    Ok(())
}

fn champ(cle: &str, noeud: &Node, cfg: &mut Config) -> Result<()> {
    let valeur = scalaire(noeud, cle)?;
    match cle {
        "check_interval" => cfg.intervalle = duree_stricte(cle, valeur)?,
        "exec_timeout" => cfg.exec_timeout = duree_stricte(cle, valeur)?,
        "webhook_timeout" => cfg.webhook_timeout = duree_stricte(cle, valeur)?,
        "log_level" => {
            cfg.niveau_log = level_from_text(valeur)
                .map_err(|_| Error::new(format!("{cle} : niveau inconnu : {valeur}")))?;
        }
        _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
    }
    Ok(())
}

fn duree_stricte(cle: &str, valeur: &str) -> Result<i64> {
    match crate::duration::parse_duration(valeur) {
        Ok(d) if d > 0 => Ok(d),
        Ok(_) => Err(Error::new(format!("{cle} doit etre strictement positif"))),
        Err(_) => Err(Error::new(format!("{cle} : duree invalide : {valeur}"))),
    }
}

#[cfg(test)]
mod tests {
    use crate::log::Level;

    #[test]
    fn duree_stricte_bornes() {
        let cfg = crate::config::parse("global:\n  check_interval: 1ms\n").expect("valide");
        assert_eq!(cfg.intervalle, 1);
        let e = crate::config::parse("global:\n  check_interval: 0ms\n").expect_err("nul");
        assert!(e.to_string().contains("strictement positif"));
        let e = crate::config::parse("global:\n  check_interval: 0\n").expect_err("nul");
        assert!(e.to_string().contains("strictement positif"));
    }

    #[test]
    fn niveau_warning_normalise() {
        let cfg = crate::config::parse("global:\n  log_level: warning\n").expect("valide");
        assert_eq!(cfg.niveau_log, Level::Warn);
    }
}
