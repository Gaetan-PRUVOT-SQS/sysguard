use crate::error::{Error, Result};
use crate::yaml::Node;

use super::Action;
use super::actions;
use super::scalaire;

#[derive(Clone, Debug, PartialEq)]
pub struct Affinite {
    pub nom: String,
    pub coeurs: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Demotion {
    pub nice: i32,
    pub patterns: Vec<String>,
    pub exceptions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cpu {
    pub gouverneur: Option<String>,
    pub epp: Option<String>,
    pub affinites: Vec<Affinite>,
    pub critique_seuil: f64,
    pub critique_duree: i64,
    pub critique_sortie: f64,
    pub soutenue_seuil: f64,
    pub soutenue_duree: i64,
    pub demotion: Option<Demotion>,
    pub actions_critique: Vec<Action>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            gouverneur: None,
            epp: None,
            affinites: Vec::new(),
            critique_seuil: 80.0,
            critique_duree: 30_000,
            critique_sortie: 50.0,
            soutenue_seuil: 40.0,
            soutenue_duree: 60_000,
            demotion: None,
            actions_critique: Vec::new(),
        }
    }
}

/// "0-3,8" → [0,1,2,3,8] ; "2" → [2]
pub fn parse_coeurs(texte: &str) -> Result<Vec<usize>> {
    let mut coeurs = Vec::new();
    for morceau in texte.split(',') {
        let morceau = morceau.trim();
        if let Some((debut, fin)) = morceau.split_once('-') {
            let d: usize = debut
                .trim()
                .parse()
                .map_err(|_| Error::new(format!("coeurs invalide : {texte}")))?;
            let f: usize = fin
                .trim()
                .parse()
                .map_err(|_| Error::new(format!("coeurs invalide : {texte}")))?;
            if f < d {
                return Err(Error::new(format!("coeurs invalide : {texte}")));
            }
            for c in d..=f {
                coeurs.push(c);
            }
        } else {
            let c: usize = morceau
                .parse()
                .map_err(|_| Error::new(format!("coeurs invalide : {texte}")))?;
            coeurs.push(c);
        }
    }
    if coeurs.is_empty() {
        return Err(Error::new(format!("coeurs invalide : {texte}")));
    }
    Ok(coeurs)
}

pub fn parse(carte: &Node) -> Result<Cpu> {
    let Node::Map(entries) = carte else {
        return Err(Error::new("cpu : carte attendue"));
    };
    let mut c = Cpu::default();
    for (cle, noeud) in entries {
        match cle.as_str() {
            "gouverneur" => c.gouverneur = Some(scalaire(noeud, cle)?.to_string()),
            "epp" => c.epp = Some(scalaire(noeud, cle)?.to_string()),
            "affinites" => c.affinites = parse_affinites(noeud)?,
            "critique_seuil" => c.critique_seuil = pourcentage(cle, scalaire(noeud, cle)?)?,
            "critique_duree" => c.critique_duree = duree_stricte(cle, scalaire(noeud, cle)?)?,
            "critique_sortie" => c.critique_sortie = pourcentage(cle, scalaire(noeud, cle)?)?,
            "soutenue_seuil" => c.soutenue_seuil = pourcentage(cle, scalaire(noeud, cle)?)?,
            "soutenue_duree" => c.soutenue_duree = duree_stricte(cle, scalaire(noeud, cle)?)?,
            "demotion" => c.demotion = Some(parse_demotion(noeud)?),
            "actions_critique" => {
                let mut liste = None;
                actions::parse(noeud, &mut liste)?;
                c.actions_critique = liste.unwrap_or_default();
            }
            _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
        }
    }
    if c.critique_sortie >= c.critique_seuil {
        return Err(Error::new(
            "critique_sortie doit etre sous critique_seuil (hysteresis)",
        ));
    }
    Ok(c)
}

fn parse_affinites(noeud: &Node) -> Result<Vec<Affinite>> {
    let Node::List(items) = noeud else {
        return Err(Error::new("affinites doit etre une liste"));
    };
    let mut liste = Vec::new();
    for item in items {
        let Node::Map(entries) = item else {
            return Err(Error::new("affinite : carte attendue"));
        };
        let mut nom = None;
        let mut coeurs = None;
        for (cle, noeud) in entries {
            let valeur = scalaire(noeud, cle)?;
            match cle.as_str() {
                "nom" => nom = Some(valeur.to_string()),
                "coeurs" => coeurs = Some(parse_coeurs(valeur)?),
                _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
            }
        }
        let Some(nom) = nom else {
            return Err(Error::new("affinite : nom requis"));
        };
        let Some(coeurs) = coeurs else {
            return Err(Error::new("affinite : coeurs requis"));
        };
        liste.push(Affinite { nom, coeurs });
    }
    Ok(liste)
}

fn parse_demotion(carte: &Node) -> Result<Demotion> {
    let Node::Map(entries) = carte else {
        return Err(Error::new("demotion : carte attendue"));
    };
    let mut d = Demotion {
        nice: 19,
        patterns: Vec::new(),
        exceptions: Vec::new(),
    };
    for (cle, noeud) in entries {
        match cle.as_str() {
            "nice" => {
                let valeur = scalaire(noeud, cle)?;
                match valeur.parse::<i32>() {
                    Ok(n) if (1..=19).contains(&n) => d.nice = n,
                    _ => {
                        return Err(Error::new(format!(
                            "nice doit etre un entier de 1 a 19 : {valeur}"
                        )));
                    }
                }
            }
            "patterns" => d.patterns = liste_scalaires(noeud, cle)?,
            "exceptions" => d.exceptions = liste_scalaires(noeud, cle)?,
            _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
        }
    }
    if d.patterns.is_empty() {
        return Err(Error::new("demotion : au moins un pattern requis"));
    }
    Ok(d)
}

fn liste_scalaires(noeud: &Node, cle: &str) -> Result<Vec<String>> {
    let Node::List(items) = noeud else {
        return Err(Error::new(format!("{cle} doit etre une liste")));
    };
    let mut liste = Vec::new();
    for item in items {
        liste.push(scalaire(item, cle)?.to_string());
    }
    Ok(liste)
}

fn duree_stricte(cle: &str, valeur: &str) -> Result<i64> {
    match crate::duration::parse_duration(valeur) {
        Ok(d) if d > 0 => Ok(d),
        Ok(_) => Err(Error::new(format!("{cle} doit etre strictement positif"))),
        Err(_) => Err(Error::new(format!("{cle} : duree invalide : {valeur}"))),
    }
}

fn pourcentage(cle: &str, valeur: &str) -> Result<f64> {
    match valeur.parse::<f64>() {
        Ok(v) if (0.0..=100.0).contains(&v) && v.is_finite() => Ok(v),
        _ => Err(Error::new(format!(
            "{cle} doit etre un pourcentage : {valeur}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeurs_ranges_et_simples() {
        assert_eq!(parse_coeurs("0-3,8").expect("valide"), vec![0, 1, 2, 3, 8]);
        assert_eq!(parse_coeurs("2").expect("valide"), vec![2]);
        assert_eq!(parse_coeurs("4-4").expect("valide"), vec![4]);
        assert!(parse_coeurs("").is_err());
        assert!(parse_coeurs("3-1").is_err());
        assert!(parse_coeurs("a-b").is_err());
    }

    #[test]
    fn defauts_symetriques_memoire() {
        let c = Cpu::default();
        assert_eq!(c.critique_seuil, 80.0);
        assert_eq!(c.critique_duree, 30_000);
        assert_eq!(c.critique_sortie, 50.0);
        assert_eq!(c.soutenue_seuil, 40.0);
        assert_eq!(c.demotion, None);
        assert_eq!(c.gouverneur, None);
    }

    #[test]
    fn parse_section_complete() {
        let yaml = "cpu:\n  gouverneur: schedutil\n  affinites:\n    - nom: monscript\n      coeurs: \"0-1\"\n  critique_seuil: 90\n  critique_duree: 20s\n  critique_sortie: 60\n  soutenue_seuil: 30\n  soutenue_duree: 45s\n  demotion:\n    nice: 15\n    patterns:\n      - montraitement\n    exceptions:\n      - sshd\n";
        let racine = crate::yaml::parse(yaml).expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        let c = parse(carte).expect("valide");
        assert_eq!(c.gouverneur, Some("schedutil".to_string()));
        assert_eq!(c.affinites.len(), 1);
        assert_eq!(c.affinites[0].coeurs, vec![0, 1]);
        assert_eq!(c.critique_seuil, 90.0);
        assert_eq!(c.soutenue_duree, 45_000);
        let d = c.demotion.expect("demotion");
        assert_eq!(d.nice, 15);
        assert_eq!(d.patterns, vec!["montraitement".to_string()]);
        assert_eq!(d.exceptions, vec!["sshd".to_string()]);
    }

    #[test]
    fn refus_section_cpu() {
        let racine = crate::yaml::parse("cpu:\n  pigeon: 1\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        assert!(parse(carte).is_err());

        let racine =
            crate::yaml::parse("cpu:\n  demotion:\n    nice: 25\n    patterns:\n      - x\n")
                .expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        assert!(parse(carte).is_err(), "nice hors plage");

        let racine = crate::yaml::parse("cpu:\n  demotion:\n    nice: 10\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        let e = parse(carte).expect_err("patterns requis");
        assert!(e.to_string().contains("pattern"), "{e}");

        let racine = crate::yaml::parse("cpu: 5\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        assert!(parse(carte).is_err());

        let racine = crate::yaml::parse("cpu:\n  affinites:\n    - nom: a\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "cpu").expect("section");
        let e = parse(carte).expect_err("coeurs requis");
        assert!(e.to_string().contains("coeurs requis"), "{e}");
    }
}
