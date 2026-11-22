use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use crate::action::{ContexteAction, Lanceur};
use crate::collect::{Collecteur, SourceStatfs};
use crate::config::{self, Config};
use crate::error::Result;
use crate::eval::{Evaluation, Moteur};
use crate::log::{Level, Logger};
use crate::notify;

pub struct Daemon<'a, W: Write> {
    chemin: PathBuf,
    once: bool,
    journal: Logger<W>,
    cfg: Config,
    collecteur: Collecteur<'a>,
    moteur: Moteur,
    ctx: ContexteAction,
    delai_watch: i64,
    arret: Arc<AtomicBool>,
    rechargement: Arc<AtomicBool>,
}

impl<'a, W: Write> Daemon<'a, W> {
    pub fn nouveau(
        chemin: PathBuf,
        once: bool,
        dry_run: bool,
        journal: Logger<W>,
        source: &'a dyn SourceStatfs,
        arret: Arc<AtomicBool>,
        rechargement: Arc<AtomicBool>,
    ) -> Self {
        Self {
            chemin,
            once,
            journal,
            cfg: Config::default(),
            collecteur: Collecteur::nouveau(source),
            moteur: Moteur::nouveau(0),
            ctx: ContexteAction {
                dry_run,
                hostname: hostname(),
            },
            delai_watch: 0,
            arret,
            rechargement,
        }
    }

    pub fn collecteur_mut(&mut self) -> &mut Collecteur<'a> {
        &mut self.collecteur
    }

    pub fn charger(&mut self) -> Result<()> {
        let cfg2 = config::charge(&self.chemin)
            .inspect_err(|e| self.journal.write(Level::Error, &e.to_string()))?;
        if self.moteur.etats.len() != cfg2.regles.len() {
            self.moteur = Moteur::nouveau(cfg2.regles.len());
        }
        self.cfg = cfg2;
        Ok(())
    }

    pub fn passe(&mut self) {
        if self.collecteur.tick(&self.cfg).is_err() {
            self.journal.write(Level::Error, "collecte impossible");
            return;
        }
        let mut lanceur = Lanceur {
            journal: &mut self.journal,
            cfg: &self.cfg,
            ctx: &self.ctx,
        };
        let mut ev = Evaluation {
            cfg: &self.cfg,
            mesures: &self.collecteur.mesures,
            moteur: &mut self.moteur,
            agir: &mut lanceur,
        };
        let _ = ev.tick();
        if self.delai_watch > 0 {
            let _ = notify::envoi("WATCHDOG=1");
        }
    }

    fn installer_signaux(&self) -> std::io::Result<()> {
        signal_hook::flag::register(SIGTERM, Arc::clone(&self.arret))?;
        signal_hook::flag::register(SIGINT, Arc::clone(&self.arret))?;
        signal_hook::flag::register(SIGHUP, Arc::clone(&self.rechargement))?;
        Ok(())
    }

    pub fn boucle(&mut self) {
        while !self.arret.load(Ordering::Relaxed) {
            if self.rechargement.swap(false, Ordering::Relaxed) {
                if self.charger().is_ok() {
                    self.journal.write(Level::Info, "configuration rechargée");
                } else {
                    self.journal
                        .write(Level::Error, "rechargement refusé, configuration conservée");
                }
            }
            self.passe();
            let intervalle = self.cfg.intervalle;
            thread::sleep(Duration::from_millis(intervalle.max(0) as u64));
        }
    }

    pub fn demarre(&mut self) -> i32 {
        if self.charger().is_err() {
            return 1;
        }
        self.journal.write(Level::Info, "daemon démarré");
        let _ = notify::envoi("READY=1");
        self.delai_watch = notify::delai_watchdog();
        if let Err(e) = self.installer_signaux() {
            self.journal
                .write(Level::Error, &format!("signaux indisponibles : {e}"));
            return 1;
        }
        if self.once {
            self.passe();
        } else {
            self.boucle();
        }
        let _ = notify::envoi("STOPPING=1");
        self.journal.write(Level::Info, "daemon arrêté");
        0
    }
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::StatfsInfo;
    use std::io;

    struct SourceNeutre;

    impl SourceStatfs for SourceNeutre {
        fn statfs(&self, _: &str) -> io::Result<StatfsInfo> {
            Err(io::Error::other("neutre"))
        }
    }

    #[test]
    fn hostname_lu() {
        let h = hostname();
        assert!(!h.is_empty(), "hostname présent sur Linux");
        assert!(!h.contains('\n'));
    }

    fn daemon_neutre(chemin: PathBuf, once: bool) -> Daemon<'static, Vec<u8>> {
        Daemon::nouveau(
            chemin,
            once,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &SourceNeutre,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn charger_echec_journalise() {
        let mut d = daemon_neutre(PathBuf::from("/inexistant/sysguard.yaml"), false);
        assert!(d.charger().is_err(), "config absente");
        let out = String::from_utf8(d.journal.contenu()).expect("utf-8");
        assert!(out.contains("fichier illisible"), "{out}");
    }

    #[test]
    fn passe_collecte_impossible() {
        let mut d = daemon_neutre(PathBuf::from("/etc/sysguard/sysguard.yaml"), false);
        d.cfg = crate::config::Config::default();
        d.collecteur_mut().chemins.stat = PathBuf::from("/inexistant-stat");
        d.passe();
        let out = String::from_utf8(d.journal.contenu()).expect("utf-8");
        assert!(out.contains("collecte impossible"), "{out}");
    }

    #[test]
    fn signaux_installes() {
        let d = daemon_neutre(PathBuf::from("/etc/sysguard/sysguard.yaml"), false);
        d.installer_signaux().expect("signaux");
    }

    #[test]
    fn boucle_s_arrete_sur_drapeau() {
        let mut d = Daemon::nouveau(
            PathBuf::from("/inexistant/sysguard.yaml"),
            false,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &SourceNeutre,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
        );
        let debut = std::time::Instant::now();
        d.boucle();
        assert!(
            debut.elapsed() < Duration::from_millis(200),
            "arrêt immédiat sans tick"
        );
        let out = String::from_utf8(d.journal.contenu()).expect("utf-8");
        assert!(out.is_empty(), "aucun journal hors signaux : {out}");
    }

    #[test]
    fn demarre_once_passe_une_fois() {
        let valide =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/daemon-once.yaml");
        let mut d = daemon_neutre(valide, true);
        assert_eq!(d.demarre(), 0);
        let out = String::from_utf8(d.journal.contenu()).expect("utf-8");
        assert!(out.contains("daemon démarré"), "{out}");
        assert!(out.contains("daemon arrêté"), "{out}");
    }
}
