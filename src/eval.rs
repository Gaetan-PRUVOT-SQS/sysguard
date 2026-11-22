use std::sync::OnceLock;
use std::time::Instant;

use crate::collect::{Disque, Mesures};
use crate::config::{Condition, Config, Metrique, Operateur, Regle};
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub struct Etat {
    pub depuis: i64,
    pub dernier: i64,
}

pub struct Moteur {
    pub etats: Vec<Etat>,
    horloge: Box<dyn Fn() -> i64>,
}

impl Moteur {
    pub fn nouveau(nb_regles: usize) -> Self {
        Self::avec_horloge(nb_regles, Box::new(maintenant_ms))
    }

    pub fn avec_horloge(nb_regles: usize, horloge: Box<dyn Fn() -> i64>) -> Self {
        Self {
            etats: vec![
                Etat {
                    depuis: -1,
                    dernier: -1,
                };
                nb_regles
            ],
            horloge,
        }
    }
}

pub fn maintenant_ms() -> i64 {
    static DEBUT: OnceLock<Instant> = OnceLock::new();
    DEBUT.get_or_init(Instant::now).elapsed().as_millis() as i64
}

pub fn comparer(valeur: f64, operateur: Option<Operateur>, seuil: f64) -> bool {
    let Some(op) = operateur else {
        return false;
    };
    match op {
        Operateur::Inf => valeur < seuil,
        Operateur::InfEgal => valeur <= seuil,
        Operateur::Sup => valeur > seuil,
        Operateur::SupEgal => valeur >= seuil,
        Operateur::Egal => valeur == seuil,
    }
}

pub fn mesure_valeur(mesures: &Mesures, condition: &Condition) -> Option<f64> {
    match condition.metrique? {
        Metrique::CpuUsage => Some(mesures.cpu_usage),
        Metrique::MemUsage => Some(mesures.mem_usage),
        Metrique::MemAvailable => Some(mesures.mem_disponible),
        Metrique::SwapUsage => Some(mesures.swap_usage),
        Metrique::Load1 => Some(mesures.load1),
        Metrique::DiskUsage => disque(mesures, condition, |d| d.usage),
        Metrique::DiskFree => disque(mesures, condition, |d| d.libre),
        Metrique::DiskInodes => disque(mesures, condition, |d| d.inodes),
    }
}

fn disque(mesures: &Mesures, condition: &Condition, champ: impl Fn(&Disque) -> f64) -> Option<f64> {
    let montage = condition.montage.as_ref()?;
    mesures
        .disques
        .iter()
        .find(|d| d.montage == *montage)
        .map(champ)
}

pub trait Agir {
    fn agir(&mut self, regle: &Regle, valeur: f64);
}

pub struct Evaluation<'a> {
    pub cfg: &'a Config,
    pub mesures: &'a Mesures,
    pub moteur: &'a mut Moteur,
    pub agir: &'a mut dyn Agir,
}

impl Evaluation<'_> {
    pub fn tick(&mut self) -> Result<()> {
        let mut echecs = 0;
        let Evaluation {
            cfg,
            mesures,
            moteur,
            agir,
        } = self;
        let Moteur { etats, horloge } = &mut **moteur;
        let horloge: &dyn Fn() -> i64 = horloge.as_ref();
        for (i, regle) in cfg.regles.iter().enumerate() {
            let Some(etat) = etats.get_mut(i) else {
                echecs += 1;
                continue;
            };
            if eval_regle(mesures, horloge, regle, etat, *agir).is_err() {
                echecs += 1;
            }
        }
        if echecs > 0 {
            return Err(Error::new("des regles sont en echec"));
        }
        Ok(())
    }
}

fn eval_regle(
    mesures: &Mesures,
    horloge: &dyn Fn() -> i64,
    regle: &Regle,
    etat: &mut Etat,
    agir: &mut dyn Agir,
) -> Result<()> {
    let Some(valeur) = mesure_valeur(mesures, &regle.condition) else {
        return Err(Error::new(format!(
            "regle {} : mesure indisponible",
            regle.nom
        )));
    };
    if !comparer(valeur, regle.condition.operateur, regle.condition.valeur) {
        etat.depuis = -1;
        return Ok(());
    }
    let maint = horloge();
    if etat.depuis < 0 {
        etat.depuis = maint;
    }
    if maint - etat.depuis < regle.pour {
        return Ok(());
    }
    if etat.dernier >= 0 && maint - etat.dernier < regle.cooldown {
        return Ok(());
    }
    etat.dernier = maint;
    agir.agir(regle, valeur);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparateurs() {
        assert!(!comparer(50.0, Some(Operateur::Inf), 50.0));
        assert!(comparer(50.0, Some(Operateur::InfEgal), 50.0));
        assert!(comparer(50.0, Some(Operateur::SupEgal), 50.0));
        assert!(comparer(50.0, Some(Operateur::Egal), 50.0));
        assert!(comparer(90.0, Some(Operateur::Sup), 50.0));
        assert!(!comparer(10.0, Some(Operateur::Sup), 50.0));
        assert!(!comparer(90.0, None, 50.0));
    }

    #[test]
    fn mesures_directes_et_disque() {
        let mut m = Mesures {
            cpu_usage: 42.0,
            ..Mesures::default()
        };
        m.disques.push(Disque {
            montage: "/".to_string(),
            usage: 75.0,
            libre: 1024.0,
            inodes: 80.0,
        });
        let c = Condition {
            metrique: Some(Metrique::CpuUsage),
            ..Condition::default()
        };
        assert_eq!(mesure_valeur(&m, &c), Some(42.0));
        let c = Condition {
            metrique: Some(Metrique::DiskFree),
            montage: Some("/".to_string()),
            ..Condition::default()
        };
        assert_eq!(mesure_valeur(&m, &c), Some(1024.0));
        let c = Condition {
            metrique: Some(Metrique::DiskUsage),
            montage: Some("/absente".to_string()),
            ..Condition::default()
        };
        assert_eq!(mesure_valeur(&m, &c), None);
        let c = Condition {
            metrique: Some(Metrique::DiskUsage),
            montage: None,
            ..Condition::default()
        };
        assert_eq!(mesure_valeur(&m, &c), None);
        assert_eq!(mesure_valeur(&m, &Condition::default()), None);
    }

    #[test]
    fn moteur_initialise_etat_neuf() {
        let mo = Moteur::nouveau(2);
        assert_eq!(mo.etats.len(), 2);
        assert_eq!(mo.etats[0].depuis, -1);
        assert_eq!(mo.etats[1].dernier, -1);
    }
}

#[cfg(test)]
mod scenarios {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    struct FauxAgir {
        tirs: Cell<i64>,
    }

    impl Agir for FauxAgir {
        fn agir(&mut self, _: &Regle, _: f64) {
            self.tirs.set(self.tirs.get() + 1);
        }
    }

    struct Banc {
        cfg: Config,
        mesures: Mesures,
        moteur: Moteur,
        temps: Rc<Cell<i64>>,
        agir: FauxAgir,
    }

    impl Banc {
        fn nouveau(yaml: &str) -> Self {
            let cfg = crate::config::parse(yaml).expect("config valide");
            let nb = cfg.regles.len();
            let temps = Rc::new(Cell::new(0i64));
            let t = Rc::clone(&temps);
            let moteur = Moteur::avec_horloge(nb, Box::new(move || t.get()));
            Self {
                cfg,
                mesures: Mesures::default(),
                moteur,
                temps,
                agir: FauxAgir { tirs: Cell::new(0) },
            }
        }

        fn tick(&mut self, temps: i64) -> Result<()> {
            self.temps.set(temps);
            let mut ev = Evaluation {
                cfg: &self.cfg,
                mesures: &self.mesures,
                moteur: &mut self.moteur,
                agir: &mut self.agir,
            };
            ev.tick()
        }

        fn tirs(&self) -> i64 {
            self.agir.tirs.get()
        }
    }

    const YAML_CPU: &str = "rules:\n  - name: cpu\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 50\n    for: 5s\n    cooldown: 10m\n    actions:\n      - type: log\n";

    const YAML_COOLDOWN: &str = "rules:\n  - name: cyc\n    condition:\n      metric: cpu.usage\n      operator: \">\"\n      value: 50\n    for: 0s\n    cooldown: 5s\n    actions:\n      - type: log\n";

    #[test]
    fn fenetre_for_et_cooldown() {
        let mut b = Banc::nouveau(YAML_CPU);
        b.mesures.cpu_usage = 90.0;
        b.tick(1000).expect("tick 1");
        assert_eq!(b.tirs(), 0, "pas de tir avant for");
        b.tick(5999).expect("tick 2");
        assert_eq!(b.tirs(), 0, "pas de tir à for - 1");
        b.tick(6000).expect("tick 3");
        assert_eq!(b.tirs(), 1, "tir à la borne de for");
        b.tick(7000).expect("tick 4");
        assert_eq!(b.tirs(), 1, "cooldown bloque");
        b.mesures.cpu_usage = 10.0;
        b.tick(8000).expect("tick 5");
        b.mesures.cpu_usage = 90.0;
        b.tick(9000).expect("tick 6");
        assert_eq!(b.tirs(), 1, "fenêtre reprise");
        b.tick(616_000).expect("tick 7");
        assert_eq!(b.tirs(), 2, "second tir après cooldown");
    }

    #[test]
    fn cooldown_periodique() {
        let mut b = Banc::nouveau(YAML_COOLDOWN);
        b.mesures.cpu_usage = 90.0;
        b.tick(0).expect("tick 0");
        assert_eq!(b.tirs(), 1, "for nul tire immédiatement");
        b.tick(4999).expect("tick 4999");
        assert_eq!(b.tirs(), 1, "cooldown pas écoulé");
        b.tick(5000).expect("tick 5000");
        assert_eq!(b.tirs(), 2, "re-tir au cooldown exact");
        b.tick(9999).expect("tick 9999");
        assert_eq!(b.tirs(), 2, "cooldown toujours actif");
        b.tick(10000).expect("tick 10000");
        assert_eq!(b.tirs(), 3, "troisième tir périodique");
    }

    #[test]
    fn horloge_reelle_tire_de_suite() {
        let cfg = crate::config::parse(YAML_COOLDOWN).expect("config");
        let mesures = Mesures {
            cpu_usage: 90.0,
            ..Mesures::default()
        };
        let mut moteur = Moteur::nouveau(cfg.regles.len());
        let mut agir = FauxAgir { tirs: Cell::new(0) };
        let mut ev = Evaluation {
            cfg: &cfg,
            mesures: &mesures,
            moteur: &mut moteur,
            agir: &mut agir,
        };
        ev.tick().expect("tick horloge réelle");
        assert_eq!(agir.tirs.get(), 1, "tir avec horloge réelle");
    }

    #[test]
    fn comparateurs_multi_regles() {
        let yaml = "rules:\n  - name: inf\n    condition:\n      metric: cpu.usage\n      operator: \"<\"\n      value: 50\n    for: 0s\n    actions:\n      - type: log\n  - name: infe\n    condition:\n      metric: cpu.usage\n      operator: \"<=\"\n      value: 50\n    for: 0s\n    actions:\n      - type: log\n  - name: supe\n    condition:\n      metric: cpu.usage\n      operator: \">=\"\n      value: 50\n    for: 0s\n    actions:\n      - type: log\n  - name: egal\n    condition:\n      metric: cpu.usage\n      operator: \"==\"\n      value: 50\n    for: 0s\n    actions:\n      - type: log\n  - name: mem\n    condition:\n      metric: mem.usage\n      operator: \"<\"\n      value: 999\n    for: 0s\n    actions:\n      - type: log\n  - name: dispo\n    condition:\n      metric: mem.available\n      operator: \">\"\n      value: 0\n    for: 0s\n    actions:\n      - type: log\n  - name: swap\n    condition:\n      metric: swap.usage\n      operator: \">=\"\n      value: 0\n    for: 0s\n    actions:\n      - type: log\n  - name: charge\n    condition:\n      metric: load.load1\n      operator: \">=\"\n      value: 0\n    for: 0s\n    actions:\n      - type: log\n";
        let mut b = Banc::nouveau(yaml);
        b.mesures.cpu_usage = 50.0;
        b.mesures.mem_usage = 0.0;
        b.mesures.mem_disponible = 5.0;
        b.mesures.swap_usage = 0.0;
        b.mesures.load1 = 0.0;
        b.tick(0).expect("tick égalité");
        assert_eq!(b.tirs(), 7, "égalité vraie plus métriques");

        b.mesures.cpu_usage = 10.0;
        b.agir.tirs.set(0);
        b.tick(1000).expect("tick dix");
        assert_eq!(b.tirs(), 6, "inf et infe vrais à dix");

        b.mesures.cpu_usage = 90.0;
        b.agir.tirs.set(0);
        b.tick(2000).expect("tick quatre-vingt-dix");
        assert_eq!(b.tirs(), 5, "supe et métriques à quatre-vingt-dix");
    }

    #[test]
    fn mesure_indisponible_et_fenetre() {
        let yaml = "rules:\n  - name: disque\n    condition:\n      metric: disk.usage\n      mount: \"/absente\"\n      operator: \">\"\n      value: 1\n    for: 1s\n    actions:\n      - type: log\n  - name: libre\n    condition:\n      metric: disk.free\n      mount: \"/present\"\n      operator: \">\"\n      value: 999999999\n    for: 0s\n    actions:\n      - type: log\n  - name: inodes\n    condition:\n      metric: disk.inodes\n      mount: \"/present\"\n      operator: \">\"\n      value: 999999999\n    for: 0s\n    actions:\n      - type: log\n";
        let mut b = Banc::nouveau(yaml);
        b.mesures.disques.push(Disque {
            montage: "/present".to_string(),
            usage: 0.0,
            libre: 102_400.0,
            inodes: 80.0,
        });
        assert!(b.tick(0).is_err(), "montage absent en erreur");
        assert_eq!(b.tirs(), 0, "pas de tir sans mesure");

        b.mesures.disques.push(Disque {
            montage: "/absente".to_string(),
            usage: 99.0,
            libre: 0.0,
            inodes: 0.0,
        });
        b.tick(1000).expect("présente sans tir");
        assert_eq!(b.tirs(), 0);
        b.tick(6000).expect("mesure après for");
        assert_eq!(b.tirs(), 1, "fenêtre survit à l'erreur");
    }
}
