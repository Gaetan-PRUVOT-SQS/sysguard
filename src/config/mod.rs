pub mod actions;
pub mod condition;
pub mod global;
pub mod optimisation;
pub use optimisation::Optimisation;
pub mod regles;

use std::path::Path;

use crate::error::{Error, Result};
use crate::file;
use crate::log::Level;
use crate::yaml::{self, Node};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metrique {
    CpuUsage,
    MemUsage,
    MemAvailable,
    SwapUsage,
    Load1,
    DiskUsage,
    DiskFree,
    DiskInodes,
}

impl Metrique {
    pub fn depuis_texte(valeur: &str) -> Result<Self> {
        match valeur {
            "cpu.usage" => Ok(Self::CpuUsage),
            "mem.usage" => Ok(Self::MemUsage),
            "mem.available" => Ok(Self::MemAvailable),
            "swap.usage" => Ok(Self::SwapUsage),
            "load.load1" => Ok(Self::Load1),
            "disk.usage" => Ok(Self::DiskUsage),
            "disk.free" => Ok(Self::DiskFree),
            "disk.inodes" => Ok(Self::DiskInodes),
            _ => Err(Error::new(format!("metrique inconnue : {valeur}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operateur {
    Inf,
    InfEgal,
    Sup,
    SupEgal,
    Egal,
}

impl Operateur {
    pub fn depuis_texte(valeur: &str) -> Result<Self> {
        match valeur {
            "<" => Ok(Self::Inf),
            "<=" => Ok(Self::InfEgal),
            ">" => Ok(Self::Sup),
            ">=" => Ok(Self::SupEgal),
            "==" => Ok(Self::Egal),
            _ => Err(Error::new(format!("operateur inconnu : {valeur}"))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severite {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeAction {
    Log,
    Webhook,
    Exec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub metrique: Option<Metrique>,
    pub montage: Option<String>,
    pub operateur: Option<Operateur>,
    pub valeur: f64,
}

impl Default for Condition {
    fn default() -> Self {
        Self {
            metrique: None,
            montage: None,
            operateur: None,
            valeur: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub kind: TypeAction,
    pub timeout: Option<i64>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub message: Option<String>,
    pub niveau_log: Option<Level>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Regle {
    pub nom: String,
    pub severite: Severite,
    pub condition: Condition,
    pub pour: i64,
    pub cooldown: i64,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub intervalle: i64,
    pub exec_timeout: i64,
    pub webhook_timeout: i64,
    pub niveau_log: Level,
    pub regles: Vec<Regle>,
    pub optimisation: Option<optimisation::Optimisation>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intervalle: 15_000,
            exec_timeout: 10_000,
            webhook_timeout: 10_000,
            niveau_log: Level::Info,
            regles: Vec::new(),
            optimisation: None,
        }
    }
}

pub fn parse(texte: &str) -> Result<Config> {
    let racine = yaml::parse(texte)?;
    parse_racine(&racine)
}

pub fn charge(chemin: &Path) -> Result<Config> {
    let texte = file::read_file(chemin)
        .map_err(|_| Error::new(format!("{} : fichier illisible", chemin.display())))?;
    parse(&texte)
}

fn parse_racine(racine: &Node) -> Result<Config> {
    let mut cfg = Config::default();
    let Node::Map(entries) = racine else {
        return Err(Error::new("document YAML invalide"));
    };
    for (cle, noeud) in entries {
        match cle.as_str() {
            "global" => global::parse(noeud, &mut cfg)?,
            "rules" => regles::parse(noeud, &mut cfg)?,
            "optimisation" => cfg.optimisation = Some(optimisation::parse(noeud)?),
            _ => return Err(Error::new(format!("cle inconnue : {cle}"))),
        }
    }
    Ok(cfg)
}

pub(crate) fn scalaire<'a>(noeud: &'a Node, cle: &str) -> Result<&'a str> {
    match noeud {
        Node::Scalar(s) => Ok(s),
        _ => Err(Error::new(format!("{cle} : valeur scalaire attendue"))),
    }
}

pub(crate) fn duree_libre(cle: &str, valeur: &str) -> Result<i64> {
    match crate::duration::parse_duration(valeur) {
        Ok(d) if d >= 0 => Ok(d),
        _ => Err(Error::new(format!("{cle} : duree invalide : {valeur}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(nom: &str) -> String {
        String::from_utf8(
            std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("fixtures")
                    .join(nom),
            )
            .expect("fixture lue"),
        )
        .expect("utf-8")
    }

    #[test]
    fn defauts_sur_document_vide() {
        let cfg = parse("").expect("valide");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn rules_vide_valide() {
        let cfg = parse("rules: []\n").expect("valide");
        assert!(cfg.regles.is_empty());
    }

    #[test]
    fn global_lu() {
        let cfg = parse("global:\n  check_interval: 2s\n").expect("valide");
        assert_eq!(cfg.intervalle, 2000);
        let cfg = parse("global:\n  log_level: warning\n").expect("valide");
        assert_eq!(cfg.niveau_log, Level::Warn);
        let cfg = parse("global:\n  exec_timeout: 5s\n").expect("valide");
        assert_eq!(cfg.exec_timeout, 5000);
        let cfg = parse("global:\n  webhook_timeout: 7s\n").expect("valide");
        assert_eq!(cfg.webhook_timeout, 7000);
    }

    #[test]
    fn global_refus() {
        let e = parse("global:\n  inconnu: 1\n").expect_err("cle inconnue");
        assert!(e.to_string().contains("cle inconnue : inconnu"));
        let e = parse("global:\n  check_interval: 0\n").expect_err("nul");
        assert!(e.to_string().contains("strictement positif"));
        let e = parse("global:\n  check_interval: 15x\n").expect_err("mal forme");
        assert!(e.to_string().contains("duree invalide : 15x"));
        let e = parse("global:\n  log_level: grave\n").expect_err("niveau");
        assert!(e.to_string().contains("niveau inconnu : grave"));
        let e = parse("global: 5\n").expect_err("scalaire");
        assert!(e.to_string().contains("global : carte attendue"));
    }

    #[test]
    fn racine_refus() {
        let e = parse("patate: 1\n").expect_err("racine");
        assert!(e.to_string().contains("cle inconnue : patate"));
        let e = parse("rules: 5\n").expect_err("scalaire");
        assert!(e.to_string().contains("rules doit etre une liste"));
        let e = parse("rules:\n  - 5\n").expect_err("regle scalaire");
        assert!(e.to_string().contains("nom requis"));
        let e = parse("global:\n  check_interval: 1s\nglobal:\n  check_interval: 2s\n")
            .expect_err("duplique");
        assert!(e.to_string().contains("cle dupliquee"));
        assert!(parse("- x\n").is_err(), "racine liste refusee");
    }

    #[test]
    fn charge_fichiers() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = charge(&base.join("valide.yaml")).expect("valide");
        assert_eq!(cfg.regles.len(), 3);
        let e = charge(&base.join("absente.yaml")).expect_err("absente");
        assert!(e.to_string().contains("fichier illisible"));
        assert!(charge(&base.join("vide.yaml")).is_err(), "vide refusee");
        assert!(charge(&base).is_err(), "repertoire refuse");
    }

    #[test]
    fn valide_complet() {
        let cfg = parse(&fixture("valide.yaml")).expect("valide");
        assert_eq!(cfg.intervalle, 15_000);
        assert_eq!(cfg.niveau_log, Level::Info);
        assert_eq!(cfg.regles.len(), 3);

        let r = &cfg.regles[0];
        assert_eq!(r.nom, "high-cpu");
        assert_eq!(r.severite, Severite::Warning);
        assert_eq!(r.condition.metrique, Some(Metrique::CpuUsage));
        assert_eq!(r.condition.operateur, Some(Operateur::Sup));
        assert_eq!(r.condition.valeur, 90.0);
        assert_eq!(r.condition.montage, None);
        assert_eq!(r.pour, 120_000);
        assert_eq!(r.cooldown, 600_000);
        assert_eq!(r.actions.len(), 1);
        assert_eq!(r.actions[0].kind, TypeAction::Log);
        assert_eq!(r.actions[0].niveau_log, Some(Level::Warn));
        assert_eq!(
            r.actions[0].message,
            Some("CPU eleve depuis 2 minutes : {{.Value}}%".to_string())
        );

        let r = &cfg.regles[1];
        assert_eq!(r.severite, Severite::Critical);
        assert_eq!(r.actions[0].kind, TypeAction::Exec);
        assert_eq!(r.actions[0].timeout, Some(30_000));
        assert_eq!(
            r.actions[1].url,
            Some("https://ntfy.sh/mon-topic".to_string())
        );
    }

    #[test]
    fn valide_simple_defauts() {
        let cfg = parse(&fixture("valide-simple.yaml")).expect("valide");
        let r = &cfg.regles[0];
        assert_eq!(r.severite, Severite::Warning);
        assert_eq!(r.pour, 0);
        assert_eq!(r.cooldown, 0);
        assert_eq!(r.actions[0].niveau_log, None);
        assert_eq!(r.actions[0].timeout, None);
    }

    #[test]
    fn valide_operateurs() {
        let cfg = parse(&fixture("valide-operateurs.yaml")).expect("valide");
        assert_eq!(cfg.niveau_log, Level::Debug);
        assert_eq!(cfg.regles.len(), 3);
        assert_eq!(cfg.regles[0].condition.operateur, Some(Operateur::InfEgal));
        assert_eq!(cfg.regles[1].condition.operateur, Some(Operateur::SupEgal));
        assert_eq!(cfg.regles[2].condition.operateur, Some(Operateur::Egal));
        assert_eq!(cfg.regles[0].actions[0].niveau_log, Some(Level::Warn));
    }

    #[test]
    fn refus_config_documentes() {
        let cas: &[(&str, &str)] = &[
            ("refus-01.yaml", "nom requis"),
            ("refus-02.yaml", "nom duplique"),
            ("refus-03.yaml", "severite inconnue"),
            ("refus-04.yaml", "operateur inconnu"),
            ("refus-05.yaml", "metrique inconnue"),
            ("refus-06.yaml", "incomplete"),
            ("refus-07.yaml", "incoherente"),
            ("refus-08.yaml", "requise"),
            ("refus-09.yaml", "requise"),
            ("refus-10.yaml", "inconnu"),
            ("refus-11.yaml", "interdit"),
            ("refus-12.yaml", "invalide"),
            ("refus-13.yaml", "invalide"),
            ("refus-14.yaml", "absolu"),
            ("refus-15.yaml", "invalide"),
            ("refus-16.yaml", "inconnue"),
            ("refus-17.yaml", "nombre"),
            ("refus-18.yaml", "invalide"),
            ("refus-19.yaml", "inconnue"),
            ("refus-20.yaml", "inconnue"),
            ("refus-21.yaml", "nombre"),
            ("refus-22.yaml", "nombre"),
            ("refus-23.yaml", "nombre"),
            ("refus-24.yaml", "attendue"),
            ("refus-25.yaml", "requise"),
            ("refus-26.yaml", "interdite"),
            ("refus-27.yaml", "interdit"),
            ("refus-28.yaml", "chaine"),
            ("refus-29.yaml", "attendue"),
            ("refus-30.yaml", "attendue"),
            ("refus-31.yaml", "liste"),
            ("refus-32.yaml", "invalide"),
            ("refus-33.yaml", "chaine"),
            ("refus-34.yaml", "inconnu"),
            ("refus-35.yaml", "invalide"),
            ("refus-36.yaml", "incomplete"),
        ];
        for (chemin, attendu) in cas {
            let cfg = parse(&fixture(chemin));
            assert!(cfg.is_err(), "{chemin} : refus attendu");
            let e = cfg.expect_err("refus").to_string();
            assert!(
                e.contains(attendu),
                "{chemin} : message « {e} » sans « {attendu} »"
            );
        }
    }

    #[test]
    fn metamorphique_parse_deterministe() {
        // Deux parses du même texte donnent la même configuration (propriété).
        let texte = fixture("valide.yaml");
        let a = parse(&texte).expect("première passe");
        let b = parse(&texte).expect("seconde passe");
        assert_eq!(a, b);
    }

    #[test]
    fn supposition_erreur_config_degradee() {
        // Config avec uniquement des règles, sans global : valide, défauts.
        let cfg = parse(&fixture("valide-simple.yaml")).expect("valide");
        assert_eq!(cfg.intervalle, 15_000);
        assert_eq!(cfg.niveau_log, Level::Info);
        // document avec seulement des commentaires : équivalent à vide
        let cfg = parse("# juste un commentaire\n").expect("valide");
        assert!(cfg.regles.is_empty());
    }

    #[test]
    fn action_sans_type_vaut_log() {
        let cfg = parse("rules:\n  - name: a\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 1\n    actions:\n      - message: x\n").expect("valide");
        assert_eq!(cfg.regles[0].actions[0].kind, TypeAction::Log);
    }
}
