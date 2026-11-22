use crate::error::{Error, Result};
use crate::yaml::Node;

use super::{Condition, Metrique, Operateur, scalaire};

pub fn parse(carte: &Node, cond: &mut Condition) -> Result<()> {
    let Node::Map(entries) = carte else {
        return Err(Error::new("condition : carte attendue"));
    };
    for (cle, noeud) in entries {
        champ(cle, noeud, cond)?;
    }
    if condition_valide(cond) {
        return Ok(());
    }
    Err(Error::new("condition incomplete ou incoherente"))
}

fn champ(cle: &str, noeud: &Node, cond: &mut Condition) -> Result<()> {
    let valeur = scalaire(noeud, cle)?;
    match cle {
        "metric" => cond.metrique = Some(Metrique::depuis_texte(valeur)?),
        "operator" => cond.operateur = Some(Operateur::depuis_texte(valeur)?),
        "mount" => cond.montage = Some(valeur.to_string()),
        "value" => cond.valeur = valeur_flottante(valeur)?,
        _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
    }
    Ok(())
}

fn valeur_flottante(valeur: &str) -> Result<f64> {
    match valeur.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(Error::new(format!("value doit etre un nombre : {valeur}"))),
    }
}

pub fn condition_valide(c: &Condition) -> bool {
    let Some(metrique) = c.metrique else {
        return false;
    };
    let Some(_) = c.operateur else {
        return false;
    };
    let disque = matches!(
        metrique,
        Metrique::DiskUsage | Metrique::DiskFree | Metrique::DiskInodes
    );
    (disque && c.montage.is_some()) || (!disque && c.montage.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_numeriques() {
        assert_eq!(valeur_flottante("90").expect("entier"), 90.0);
        assert_eq!(valeur_flottante("-5.5").expect("negatif"), -5.5);
        assert_eq!(valeur_flottante("1e3").expect("exposant"), 1000.0);
        assert!(valeur_flottante("").is_err());
        assert!(valeur_flottante("1x").is_err());
        assert!(valeur_flottante("1e999").is_err(), "infini refuse");
        assert!(valeur_flottante("nan").is_err(), "nan refuse");
    }

    #[test]
    fn coherence_disque() {
        let disque = Condition {
            metrique: Some(Metrique::DiskUsage),
            operateur: Some(Operateur::Sup),
            ..Condition::default()
        };
        assert!(!condition_valide(&disque), "mount requis");
        let avec_mount = Condition {
            montage: Some("/".to_string()),
            ..disque
        };
        assert!(condition_valide(&avec_mount));
        let cpu = Condition {
            metrique: Some(Metrique::CpuUsage),
            operateur: Some(Operateur::Sup),
            montage: Some("/".to_string()),
            ..Condition::default()
        };
        assert!(!condition_valide(&cpu), "mount interdit hors disque");
        let cpu_sans_mount = Condition {
            montage: None,
            ..cpu
        };
        assert!(condition_valide(&cpu_sans_mount));
    }
}
