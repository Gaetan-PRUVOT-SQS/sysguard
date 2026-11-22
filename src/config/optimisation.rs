use crate::error::{Error, Result};
use crate::yaml::Node;

use super::actions;
use super::{Action, scalaire};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZswapMode {
    Garde,
    Desactive,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Optimisation {
    pub intervalle: i64,
    pub swappiness: Option<i64>,
    pub page_cluster: Option<i64>,
    pub zram_algorithme: Option<String>,
    pub zram_provisionner: bool,
    pub zram_taille_mib: Option<i64>,
    pub zswap: ZswapMode,
    pub critique_seuil: f64,
    pub critique_duree: i64,
    pub critique_sortie: f64,
    pub soutenue_seuil: f64,
    pub soutenue_duree: i64,
    pub actions_critique: Vec<Action>,
}

impl Default for Optimisation {
    fn default() -> Self {
        Self {
            intervalle: 10_000,
            swappiness: None,
            page_cluster: None,
            zram_algorithme: None,
            zram_provisionner: false,
            zram_taille_mib: None,
            zswap: ZswapMode::Garde,
            critique_seuil: 60.0,
            critique_duree: 30_000,
            critique_sortie: 40.0,
            soutenue_seuil: 20.0,
            soutenue_duree: 60_000,
            actions_critique: Vec::new(),
        }
    }
}

pub fn parse(carte: &Node) -> Result<Optimisation> {
    let Node::Map(entries) = carte else {
        return Err(Error::new("optimisation : carte attendue"));
    };
    let mut o = Optimisation::default();
    for (cle, noeud) in entries {
        match cle.as_str() {
            "intervalle" => o.intervalle = duree_stricte(cle, scalaire(noeud, cle)?)?,
            "swappiness" => {
                o.swappiness = Some(reglage_entier(cle, scalaire(noeud, cle)?, 0, 200)?)
            }
            "page_cluster" => {
                o.page_cluster = Some(reglage_entier(cle, scalaire(noeud, cle)?, 0, 32)?)
            }
            "zram_algorithme" => o.zram_algorithme = Some(scalaire(noeud, cle)?.to_string()),
            "zram_provisionner" => {
                o.zram_provisionner = booleen(cle, scalaire(noeud, cle)?)?;
            }
            "zram_taille" => {
                o.zram_taille_mib = taille_mib(cle, scalaire(noeud, cle)?)?;
            }
            "zswap" => {
                o.zswap = match scalaire(noeud, cle)? {
                    "garde" => ZswapMode::Garde,
                    "desactive" => ZswapMode::Desactive,
                    autre => {
                        return Err(Error::new(format!(
                            "zswap doit etre garde ou desactive : {autre}"
                        )));
                    }
                };
            }
            "critique_seuil" => o.critique_seuil = pourcentage(cle, scalaire(noeud, cle)?)?,
            "critique_duree" => o.critique_duree = duree_stricte(cle, scalaire(noeud, cle)?)?,
            "critique_sortie" => o.critique_sortie = pourcentage(cle, scalaire(noeud, cle)?)?,
            "soutenue_seuil" => o.soutenue_seuil = pourcentage(cle, scalaire(noeud, cle)?)?,
            "soutenue_duree" => o.soutenue_duree = duree_stricte(cle, scalaire(noeud, cle)?)?,
            "actions_critique" => {
                let mut liste = None;
                actions::parse(noeud, &mut liste)?;
                o.actions_critique = liste.unwrap_or_default();
            }
            _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
        }
    }
    if o.critique_sortie >= o.critique_seuil {
        return Err(Error::new(
            "critique_sortie doit etre sous critique_seuil (hysteresis)",
        ));
    }
    Ok(o)
}

fn duree_stricte(cle: &str, valeur: &str) -> Result<i64> {
    match crate::duration::parse_duration(valeur) {
        Ok(d) if d > 0 => Ok(d),
        Ok(_) => Err(Error::new(format!("{cle} doit etre strictement positif"))),
        Err(_) => Err(Error::new(format!("{cle} : duree invalide : {valeur}"))),
    }
}

fn booleen(cle: &str, valeur: &str) -> Result<bool> {
    match valeur {
        "oui" | "vrai" | "true" => Ok(true),
        "non" | "faux" | "false" => Ok(false),
        _ => Err(Error::new(format!("{cle} doit etre oui ou non : {valeur}"))),
    }
}

fn taille_mib(cle: &str, valeur: &str) -> Result<Option<i64>> {
    if valeur == "auto" {
        return Ok(None);
    }
    match valeur.parse::<i64>() {
        Ok(mib) if (1..=65_536).contains(&mib) => Ok(Some(mib)),
        _ => Err(Error::new(format!(
            "{cle} doit etre auto ou des Mio (1 a 65536) : {valeur}"
        ))),
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

fn reglage_entier(cle: &str, valeur: &str, min: i64, max: i64) -> Result<i64> {
    if valeur == "auto" {
        return Ok(-1);
    }
    match valeur.parse::<f64>() {
        Ok(v) if v.is_finite() && v.fract() == 0.0 && v >= min as f64 && v <= max as f64 => {
            Ok(v as i64)
        }
        _ => Err(Error::new(format!(
            "{cle} doit etre un entier de {min} a {max} ou auto : {valeur}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defauts_justifies() {
        let o = Optimisation::default();
        assert_eq!(o.intervalle, 10_000);
        assert_eq!(o.critique_seuil, 60.0);
        assert_eq!(o.critique_duree, 30_000);
        assert_eq!(o.critique_sortie, 40.0);
        assert_eq!(o.soutenue_seuil, 20.0);
        assert_eq!(o.swappiness, None);
    }

    #[test]
    fn parse_section() {
        let yaml = "optimisation:\n  intervalle: 5s\n  swappiness: 150\n  page_cluster: auto\n  zram_algorithme: zstd\n  critique_seuil: 70\n  critique_duree: 20s\n  critique_sortie: 45\n  soutenue_seuil: 15\n  soutenue_duree: 45s\n";
        let racine = crate::yaml::parse(yaml).expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        let o = parse(carte).expect("valide");
        assert_eq!(o.intervalle, 5_000);
        assert_eq!(o.swappiness, Some(150));
        assert_eq!(o.page_cluster, Some(-1), "auto");
        assert_eq!(o.zram_algorithme, Some("zstd".to_string()));
        assert_eq!(o.critique_seuil, 70.0);
        assert_eq!(o.critique_duree, 20_000);
        assert_eq!(o.critique_sortie, 45.0);
        assert_eq!(o.soutenue_seuil, 15.0);
        assert_eq!(o.soutenue_duree, 45_000);
    }

    #[test]
    fn actions_critique_liste_acceptee() {
        let yaml = "optimisation:\n  actions_critique:\n    - type: log\n      message: \"pression {{.Value}}%\"\n    - type: exec\n      command: /bin/true\n";
        let racine = crate::yaml::parse(yaml).expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        let o = parse(carte).expect("valide");
        assert_eq!(o.actions_critique.len(), 2);
        let e = parse(
            crate::yaml::cherche(
                &crate::yaml::parse("optimisation:\n  actions_critique: 5\n").expect("yaml"),
                "optimisation",
            )
            .expect("section"),
        )
        .expect_err("scalaire refuse");
        assert!(e.to_string().contains("liste"), "{e}");
    }

    #[test]
    fn refus_config_optimisation() {
        let racine = crate::yaml::parse("optimisation:\n  swappiness: 250\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        assert!(parse(carte).is_err(), "hors plage");

        let racine =
            crate::yaml::parse("optimisation:\n  critique_seuil: 40\n  critique_sortie: 50\n")
                .expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        let e = parse(carte).expect_err("hysteresis");
        assert!(e.to_string().contains("hysteresis"), "{e}");

        let racine = crate::yaml::parse("optimisation:\n  pigeon: 1\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        assert!(parse(carte).is_err());

        let racine = crate::yaml::parse("optimisation: 5\n").expect("yaml");
        let carte = crate::yaml::cherche(&racine, "optimisation").expect("section");
        assert!(parse(carte).is_err());
    }
}
