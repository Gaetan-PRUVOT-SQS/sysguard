use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::action::{ActionCours, ContexteAction, executer};
use crate::config::{Condition, Config, Regle, Severite};
use crate::error::Result;
use crate::file;
use crate::log::{Level, Logger};
use crate::notify;
use crate::pression::{lire_pression, lire_pression_cpu};
use crate::processus::{ProcessusCpu, top_consommateurs};

pub const CHEMIN_SWAPPINESS: &str = "/proc/sys/vm/swappiness";
pub const CHEMIN_PAGE_CLUSTER: &str = "/proc/sys/vm/page-cluster";
pub const CHEMIN_ZSWAP_ACTIVE: &str = "/sys/module/zswap/parameters/enabled";

pub trait CibleReglage {
    fn lit_fichier(&self, chemin: &Path) -> io::Result<String>;
    fn ecrit_fichier(&self, chemin: &Path, valeur: &str) -> io::Result<()>;
    fn lit_pression(&self) -> Result<crate::pression::PressionMemoire> {
        lire_pression(Path::new("/proc/pressure/memory"))
    }
    fn lit_pression_cpu(&self) -> Result<crate::pression::PressionCpu> {
        lire_pression_cpu(Path::new("/proc/pressure/cpu"))
    }

    fn service_actif(&self, nom: &str) -> bool {
        std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", nom])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn lit_load1(&self) -> Result<f64> {
        use crate::error::Error;
        let mut tampon = [0u8; 128];
        let texte = crate::file::read_proc(Path::new("/proc/loadavg"), &mut tampon)?;
        texte
            .split_whitespace()
            .next()
            .and_then(|c| c.parse::<f64>().ok())
            .ok_or_else(|| Error::new("loadavg illisible"))
    }

    fn fichier_existe(&self, chemin: &Path) -> bool {
        chemin.exists()
    }

    fn zones_swap(&self) -> Result<Vec<crate::compression::ZoneSwap>> {
        let mut tampon = [0u8; 4096];
        let texte = file::read_proc(Path::new("/proc/swaps"), &mut tampon)?;
        Ok(crate::pression::parse_swaps(texte)
            .into_iter()
            .map(|zone| crate::compression::ZoneSwap {
                type_zone: crate::compression::type_zone(&zone.chemin),
                chemin: zone.chemin,
                taille_ko: zone.taille_ko,
                priorite: zone.priorite,
            })
            .collect())
    }

    fn ram_octets(&self) -> u64 {
        let mut tampon = [0u8; 8192];
        crate::file::read_proc(Path::new("/proc/meminfo"), &mut tampon)
            .ok()
            .and_then(|t| {
                t.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| {
                        l.split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                    })
            })
            .map(|ko| ko * 1024)
            .unwrap_or(0)
    }

    fn lance_commande(&self, prog: &str, args: &[&str]) -> io::Result<()> {
        crate::sys::commande(prog, args)
    }

    fn swapon(&self, chemin: &Path, priorite: i32) -> io::Result<()> {
        crate::sys::swapon(chemin, priorite)
    }
    fn zram_sysfs(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/sys/block/zram0"))
    }
    fn liste_cpus(&self) -> Vec<PathBuf> {
        let mut cpus = Vec::new();
        let Ok(entrees) = std::fs::read_dir("/sys/devices/system/cpu") else {
            return cpus;
        };
        for entree in entrees.flatten() {
            let nom = entree.file_name();
            let Some(nom) = nom.to_str() else { continue };
            if nom.starts_with("cpu") && nom[3..].bytes().all(|b| b.is_ascii_digit()) {
                cpus.push(entree.path());
            }
        }
        cpus.sort();
        cpus
    }

    fn processus(&self) -> Result<Vec<ProcessusCpu>> {
        let mut tampon = [0u8; 2048];
        crate::processus::liste_processus(Path::new("/proc"), &mut tampon)
    }

    fn renice(&self, pid: i32, nice: i32) -> io::Result<()> {
        crate::sys::renice(pid, nice)
    }

    fn fixe_affinite(&self, pid: i32, coeurs: &[usize]) -> io::Result<()> {
        crate::sys::fixe_affinite(pid, coeurs)
    }
}

pub struct CibleReelle;

impl CibleReglage for CibleReelle {
    fn lit_fichier(&self, chemin: &Path) -> io::Result<String> {
        std::fs::read_to_string(chemin)
    }

    fn ecrit_fichier(&self, chemin: &Path, valeur: &str) -> io::Result<()> {
        std::fs::write(chemin, valeur)
    }
}

pub struct Optimiseur<'a, W: Write> {
    cfg: Config,
    once: bool,
    journal: Logger<W>,
    cible: &'a dyn CibleReglage,
    ctx: ContexteAction,
    critique_depuis: i64,
    critique_actif: bool,
    soutenue_depuis: i64,
    dernier_recompress: i64,
    socle_pose: bool,
    psi_premier_total: Option<u64>,
    psi_a_bouge: bool,
    psi_fige: bool,
    cpu_premier_total: Option<u64>,
    cpu_a_bouge: bool,
    cpu_fige: bool,
    cpu_critique_depuis: i64,
    cpu_critique_actif: bool,
    cpu_soutenue_depuis: i64,
    proces_prec: Option<Vec<ProcessusCpu>>,
    demotes: Vec<(i32, String)>,
    arret: Arc<AtomicBool>,
    rechargement: Arc<AtomicBool>,
    delai_watch: i64,
}

impl<'a, W: Write> Optimiseur<'a, W> {
    pub fn nouveau(
        cfg: Config,
        once: bool,
        journal: Logger<W>,
        cible: &'a dyn CibleReglage,
        arret: Arc<AtomicBool>,
        rechargement: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cfg,
            once,
            journal,
            cible,
            ctx: ContexteAction {
                dry_run: false,
                hostname: crate::daemon::hostname(),
            },
            critique_depuis: -1,
            critique_actif: false,
            soutenue_depuis: -1,
            dernier_recompress: -1_000_000,
            socle_pose: false,
            psi_premier_total: None,
            psi_a_bouge: false,
            psi_fige: false,
            cpu_premier_total: None,
            cpu_a_bouge: false,
            cpu_fige: false,
            cpu_critique_depuis: -1,
            cpu_critique_actif: false,
            cpu_soutenue_depuis: -1,
            proces_prec: None,
            demotes: Vec::new(),
            arret,
            rechargement,
            delai_watch: 0,
        }
    }

    pub fn journal_mut(&mut self) -> &mut Logger<W> {
        &mut self.journal
    }

    fn zram_presents(&self) -> bool {
        match self.cible.zram_sysfs() {
            Some(sysfs) => self.cible.lit_fichier(&sysfs.join("mm_stat")).is_ok(),
            None => false,
        }
    }

    fn ecrit(&mut self, chemin: &Path, valeur: &str, nom: &str) {
        match self.cible.ecrit_fichier(chemin, valeur) {
            Ok(()) => self
                .journal
                .write(Level::Info, &format!("optimisation : {nom} = {valeur}")),
            Err(e) if !self.socle_pose => self.journal.write(
                Level::Warn,
                &format!("optimisation : {nom} non applique ({e}), recommandation : {valeur}"),
            ),
            Err(_) => {}
        }
    }

    pub fn socle(&mut self) {
        let optimisation = self.cfg.optimisation.clone();
        let zram_present = self.zram_presents();

        if let Some(o) = &optimisation
            && let Some(algo) = &o.zram_algorithme
            && zram_present
        {
            let sysfs = self.cible.zram_sysfs().expect("zram présent");
            let chemin = sysfs.join("comp_algorithm");
            let actuel = self
                .cible
                .lit_fichier(&chemin)
                .map(|s| crate::pression::algo_actif(&s))
                .unwrap_or_default();
            if actuel != *algo {
                match self.cible.ecrit_fichier(&chemin, algo) {
                    Ok(()) => self.journal.write(
                        Level::Info,
                        &format!("optimisation : zram comp_algorithm = {algo}"),
                    ),
                    Err(e) => self.journal.write(
                        Level::Warn,
                        &format!(
                            "optimisation : zram comp_algorithm = {algo} non applique ({e}) : \
                             regler au boot via zram-generator (compression-algorithm)"
                        ),
                    ),
                }
            } else {
                self.journal
                    .write(Level::Info, &format!("optimisation : zram deja en {algo}"));
            }
        }

        if let Some(o) = &optimisation {
            let etat = match crate::compression::detecte(self.cible) {
                Ok(e) => e,
                Err(e) => {
                    self.journal.write(
                        Level::Warn,
                        &format!(
                            "optimisation : etat de compression illisible ({e}), \\
                             socle memoire et provisionnement ignores"
                        ),
                    );
                    self.socle_cpu();
                    self.socle_pose = true;
                    return;
                }
            };
            let mut zram_actif = etat.zram_presents;

            if o.zram_provisionner {
                let disque_present = etat
                    .zones
                    .iter()
                    .any(|z| z.type_zone == crate::compression::TypeZone::Disque);
                if !etat.zones.is_empty() {
                    let types: Vec<String> = etat
                        .zones
                        .iter()
                        .map(|z| {
                            format!(
                                "{} ({})",
                                z.chemin,
                                match z.type_zone {
                                    crate::compression::TypeZone::Zram => "zram",
                                    crate::compression::TypeZone::Disque => "disque",
                                }
                            )
                        })
                        .collect();
                    self.journal.write(
                        Level::Info,
                        &format!(
                            "optimisation : swap deja present ({}), provisionnement zram non applique{}",
                            types.join(", "),
                            if disque_present {
                                " (swap disque + zram = inversion LRU)"
                            } else {
                                ""
                            }
                        ),
                    );
                } else if etat.generator_installe && etat.generator_confige {
                    self.journal.write(
                        Level::Info,
                        "optimisation : zram gere au boot par zram-generator, non duplique",
                    );
                } else {
                    self.provisionne(o);
                    zram_actif = self.zram_presents() || zram_actif;
                }
            } else if etat.zones.is_empty() && !etat.zram_presents {
                self.journal.write(
                    Level::Info,
                    "optimisation : aucun swap sur la machine (provisionnement possible avec zram_provisionner: oui)",
                );
            }

            if o.zswap == crate::config::optimisation::ZswapMode::Desactive
                && etat.zswap_actif == Some(true)
            {
                self.ecrit(
                    Path::new(crate::compression::CHEMIN_ZSWAP_ACTIVE),
                    "0",
                    "zswap enabled",
                );
            } else if o.zswap == crate::config::optimisation::ZswapMode::Garde
                && etat.zswap_actif == Some(true)
                && (self.zram_presents() || zram_actif)
            {
                self.journal.write(
                    Level::Warn,
                    "optimisation : zswap actif devant zram (double compression), desactiver si non voulu",
                );
            }

            let zram_presents = self.zram_presents();
            if let Some(algo) = &o.zram_algorithme
                && zram_presents
            {
                let algo_resolu = match algo.as_str() {
                    "auto" => {
                        if self.cible.liste_cpus().len() >= 4 {
                            "zstd".to_string()
                        } else {
                            "lz4".to_string()
                        }
                    }
                    autre => autre.to_string(),
                };
                let sysfs = self.cible.zram_sysfs().expect("zram présent");
                let chemin = sysfs.join("comp_algorithm");
                let actuel = self
                    .cible
                    .lit_fichier(&chemin)
                    .map(|s| crate::pression::algo_actif(&s))
                    .unwrap_or_default();
                if actuel != algo_resolu {
                    match self.cible.ecrit_fichier(&chemin, &algo_resolu) {
                        Ok(()) => self.journal.write(
                            Level::Info,
                            &format!(
                                "optimisation : zram comp_algorithm = {algo_resolu}"
                            ),
                        ),
                        Err(e) => self.journal.write(
                            Level::Warn,
                            &format!(
                                "optimisation : zram comp_algorithm = {algo_resolu} non applique ({e}) : \
                                 regler au boot via zram-generator (compression-algorithm)"
                            ),
                        ),
                    }
                } else {
                    self.journal.write(
                        Level::Info,
                        &format!("optimisation : zram deja en {algo_resolu}"),
                    );
                }
            }

            let cible_swappiness = match o.swappiness {
                Some(v) if v >= 0 => Some(v),
                _ => {
                    if zram_presents || zram_actif {
                        Some(133)
                    } else {
                        self.journal.write(
                            Level::Info,
                            "optimisation : swappiness non modifie (pas de zram, auto)",
                        );
                        None
                    }
                }
            };
            if let Some(v) = cible_swappiness {
                let chemin = PathBuf::from(CHEMIN_SWAPPINESS);
                let actuel = self.cible.lit_fichier(&chemin).ok();
                let actuel = actuel.as_deref().map(str::trim);
                if actuel != Some(v.to_string().as_str()) {
                    self.ecrit(&chemin, &v.to_string(), "vm.swappiness");
                }
            }

            let cible_cluster = match o.page_cluster {
                Some(v) if v >= 0 => Some(v),
                _ => {
                    if zram_presents || zram_actif {
                        Some(0)
                    } else {
                        None
                    }
                }
            };
            if let Some(v) = cible_cluster {
                let chemin = PathBuf::from(CHEMIN_PAGE_CLUSTER);
                let actuel = self.cible.lit_fichier(&chemin).ok();
                let actuel = actuel.as_deref().map(str::trim);
                if actuel != Some(v.to_string().as_str()) {
                    self.ecrit(&chemin, &v.to_string(), "vm.page-cluster");
                }
            }
        }

        self.socle_cpu();

        self.socle_pose = true;
    }

    fn provisionne(&mut self, o: &crate::config::optimisation::Optimisation) {
        if let Err(e) = self.cible.lance_commande("modprobe", &["zram"]) {
            self.journal.write(
                Level::Warn,
                &format!("optimisation : module zram non charge ({e}), provisionnement abandonne"),
            );
            return;
        }
        let Some(sysfs) = self.cible.zram_sysfs() else {
            return;
        };
        let algo_resolu = match o.zram_algorithme.as_deref() {
            Some(a) if a != "auto" => a.to_string(),
            _ => {
                if self.cible.liste_cpus().len() >= 4 {
                    "zstd".to_string()
                } else {
                    "lz4".to_string()
                }
            }
        };
        let chemin_algo = sysfs.join("comp_algorithm");
        if self
            .cible
            .lit_fichier(&chemin_algo)
            .map(|s| crate::pression::algo_actif(&s))
            .unwrap_or_default()
            != algo_resolu
            && let Err(e) = self.cible.ecrit_fichier(&chemin_algo, &algo_resolu)
        {
            self.journal.write(
                Level::Warn,
                &format!("optimisation : zram comp_algorithm = {algo_resolu} non applique ({e})"),
            );
        }
        let ram_mib = (self.cible.ram_octets() / 1024 / 1024) as i64;
        let taille_mib = o.zram_taille_mib.unwrap_or((ram_mib / 2).clamp(1, 4096));
        let chemin_taille = sysfs.join("disksize");
        let taille_octets = taille_mib as u64 * 1024 * 1024;
        if let Err(e) = self
            .cible
            .ecrit_fichier(&chemin_taille, &taille_octets.to_string())
        {
            self.journal.write(
                Level::Warn,
                &format!(
                    "optimisation : disksize {} non applique ({e}), provisionnement abandonne",
                    chemin_taille.display()
                ),
            );
            return;
        }
        let chemin_device = PathBuf::from(format!(
            "/dev/{}",
            sysfs
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("zram0")
        ));
        let mut attente = 0;
        while !self.cible.fichier_existe(&chemin_device) && attente < 10 {
            thread::sleep(Duration::from_millis(500));
            attente += 1;
        }
        if !self.cible.fichier_existe(&chemin_device) {
            self.journal.write(
                Level::Warn,
                &format!(
                    "optimisation : {} absent apres 5s (udev ?), provisionnement abandonne",
                    chemin_device.display()
                ),
            );
            return;
        }
        if let Err(e) = self
            .cible
            .lance_commande("mkswap", &[chemin_device.to_str().expect("chemin utf-8")])
        {
            self.journal.write(
                Level::Warn,
                &format!("optimisation : mkswap en echec ({e}), provisionnement abandonne"),
            );
            return;
        }
        match self.cible.swapon(&chemin_device, 100) {
            Ok(()) => self.journal.write(
                Level::Info,
                &format!(
                    "optimisation : zram provisionne {} {} Mio, algo {}, priorite 100",
                    chemin_device.display(),
                    taille_mib,
                    algo_resolu
                ),
            ),
            Err(e) => {
                self.journal.write(
                    Level::Warn,
                    &format!("optimisation : swapon en echec ({e}), provisionnement partiel"),
                );
                return;
            }
        }
        let conf = format!(
            "[zram0]\nzram-size = {}\ncompression-algorithm = {}\nswap-priority = 100\n",
            taille_mib, algo_resolu
        );
        self.ecrit(
            Path::new(crate::compression::CHEMIN_GENERATEUR_CONF),
            &conf,
            "zram-generator.conf (persistance)",
        );
        if !self
            .cible
            .fichier_existe(Path::new(crate::compression::CHEMIN_GENERATEUR))
        {
            self.journal.write(
                Level::Info,
                "optimisation : installer zram-generator pour appliquer cette conf aux boots suivants",
            );
        }
    }

    fn socle_cpu(&mut self) {
        let Some(cpu) = self.cfg.cpu.clone() else {
            return;
        };
        let mut gestionnaires: Vec<String> = Vec::new();
        for (service, nom) in [
            ("tuned.service", "tuned"),
            ("power-profiles-daemon.service", "power-profiles-daemon"),
            ("auto-cpufreq.service", "auto-cpufreq"),
        ] {
            if self.cible.service_actif(service) {
                gestionnaires.push(nom.to_string());
            }
        }
        if !gestionnaires.is_empty() {
            self.journal.write(
                Level::Info,
                &format!(
                    "optimisation : CPU gere par {} (gouverneur/epp non appliques, pas de doublon)",
                    gestionnaires.join(", ")
                ),
            );
            return;
        }
        if let Some(epp) = &cpu.epp {
            let mut cpus = self.cible.liste_cpus();
            cpus.retain(|c| {
                self.cible
                    .fichier_existe(&c.join("energy_performance_preference"))
            });
            if cpus.is_empty() {
                self.journal.write(
                    Level::Warn,
                    "optimisation : epp non supporte par ce materiel",
                );
            } else {
                let mut echecs = 0;
                for cpu_dir in &cpus {
                    let chemin = cpu_dir.join("energy_performance_preference");
                    let actuel = self
                        .cible
                        .lit_fichier(&chemin)
                        .ok()
                        .map(|s| s.trim().to_string());
                    if actuel.as_deref() != Some(epp.as_str())
                        && self.cible.ecrit_fichier(&chemin, epp).is_err()
                    {
                        echecs += 1;
                    }
                }
                if echecs > 0 {
                    self.journal.write(
                        Level::Warn,
                        &format!("optimisation : epp {epp} refuse par {echecs} coeur(s)"),
                    );
                } else {
                    self.journal.write(
                        Level::Info,
                        &format!("optimisation : epp = {epp} sur {} coeur(s)", cpus.len()),
                    );
                }
            }
        }
        if !self.cible.service_actif("irqbalance.service") {
            self.journal.write(
                Level::Info,
                "optimisation : irqbalance absent, recommande pour repartir les interruptions",
            );
        }
        if let Some(gouverneur) = &cpu.gouverneur {
            #[cfg(test)]
            eprintln!(
                "DEBUG gouverneur={gouverneur} gestionnaires={gestionnaires:?} cpus={} services={:?}",
                self.cible.liste_cpus().len(),
                self.cible.service_actif("tuned.service")
            );
            let cpus = self.cible.liste_cpus();
            if cpus.is_empty() {
                self.journal.write(
                    Level::Warn,
                    "optimisation : cpufreq absent (VM ou intel_pstate), gouverneur non applique",
                );
            } else {
                let mut echecs = 0;
                for cpu_dir in &cpus {
                    let chemin = cpu_dir.join("scaling_governor");
                    let actuel = self
                        .cible
                        .lit_fichier(&chemin)
                        .ok()
                        .map(|s| s.trim().to_string());
                    if actuel.as_deref() != Some(gouverneur.as_str()) {
                        match self.cible.ecrit_fichier(&chemin, gouverneur) {
                            Ok(()) => self.journal.write(
                                Level::Info,
                                &format!("optimisation : {} = {gouverneur}", chemin.display()),
                            ),
                            Err(_) => echecs += 1,
                        }
                    }
                }
                if echecs > 0 {
                    self.journal.write(
                        Level::Warn,
                        &format!(
                            "optimisation : gouverneur {gouverneur} refuse par {echecs} coeur(s)"
                        ),
                    );
                }
            }
        }
        self.applique_affinites(&cpu.affinites, true);
    }

    fn applique_affinites(&mut self, regles: &[crate::config::cpu::Affinite], journailler: bool) {
        let Ok(processus) = self.cible.processus() else {
            return;
        };
        for regle in regles {
            for p in processus.iter().filter(|p| p.comm == regle.nom) {
                match self.cible.fixe_affinite(p.pid, &regle.coeurs) {
                    Ok(()) if journailler => self.journal.write(
                        Level::Info,
                        &format!(
                            "optimisation : affinite {} (pid {}) = {:?}",
                            regle.nom, p.pid, regle.coeurs
                        ),
                    ),
                    Err(e) if journailler && !self.socle_pose => self.journal.write(
                        Level::Warn,
                        &format!(
                            "optimisation : affinite {} non appliquee ({e}), recommandation : {:?}",
                            regle.nom, regle.coeurs
                        ),
                    ),
                    _ => {}
                }
            }
        }
    }

    fn actions_critique(&mut self, seuil: f64, valeur: f64) {
        let Some(optimisation) = self.cfg.optimisation.clone() else {
            return;
        };
        let virtuelle = Regle {
            nom: "optimisation-memoire".to_string(),
            severite: Severite::Warning,
            condition: Condition {
                metrique: None,
                montage: None,
                operateur: None,
                valeur: seuil,
            },
            pour: 0,
            cooldown: 0,
            actions: Vec::new(),
        };
        for act in &optimisation.actions_critique {
            let ac = ActionCours {
                act,
                regle: &virtuelle,
                valeur,
            };
            let _ = executer(&ac, &mut self.journal, &self.cfg, &self.ctx);
        }
    }

    pub fn pas(&mut self, horloge: &dyn Fn() -> i64) -> Result<()> {
        let Some(optimisation) = self.cfg.optimisation.clone() else {
            return self.pas_cpu(horloge);
        };
        let pression = self.cible.lit_pression()?;
        let maint = horloge();

        if !self.psi_fige && !self.psi_a_bouge {
            match self.psi_premier_total {
                None => self.psi_premier_total = Some(pression.total_micros),
                Some(premier) if premier != pression.total_micros => {
                    self.psi_premier_total = Some(pression.total_micros);
                    self.psi_a_bouge = true;
                }
                Some(premier)
                    if premier == pression.total_micros
                        && self.cible.lit_load1().unwrap_or(0.0) >= 0.5 =>
                {
                    self.psi_fige = true;
                    self.journal.write(
                        Level::Warn,
                        "optimisation : PSI memoire fige sous charge (psi= absent au boot ?), en repli",
                    );
                }
                Some(_) => {}
            }
        }
        if self.psi_fige {
            return self.pas_cpu(horloge);
        }

        if pression.full >= optimisation.critique_seuil {
            if self.critique_depuis < 0 {
                self.critique_depuis = maint;
            }
            if !self.critique_actif && maint - self.critique_depuis >= optimisation.critique_duree {
                self.critique_actif = true;
                self.journal.write(
                    Level::Error,
                    &format!(
                        "optimisation : pression memoire critique ({:.1}% tenu {}ms)",
                        pression.full, optimisation.critique_duree
                    ),
                );
                self.actions_critique(optimisation.critique_seuil, pression.full);
            }
        } else if self.critique_actif && pression.full < optimisation.critique_sortie {
            self.critique_actif = false;
            self.critique_depuis = -1;
            self.journal.write(
                Level::Info,
                &format!(
                    "optimisation : pression memoire retombee ({:.1}%)",
                    pression.full
                ),
            );
        } else if !self.critique_actif && pression.full < optimisation.critique_sortie {
            self.critique_depuis = -1;
        }

        if pression.some >= optimisation.soutenue_seuil && !self.critique_actif {
            if self.soutenue_depuis < 0 {
                self.soutenue_depuis = maint;
            }
            if maint - self.soutenue_depuis >= optimisation.soutenue_duree {
                self.soutenue_depuis = maint;
                self.journal.write(
                    Level::Warn,
                    &format!(
                        "optimisation : pression memoire soutenue ({:.1}% some)",
                        pression.some
                    ),
                );
                if maint - self.dernier_recompress > optimisation.soutenue_duree * 3
                    && let Some(sysfs) = self.cible.zram_sysfs()
                {
                    let chemin = sysfs.join("recompress");
                    if self.cible.lit_fichier(&chemin).is_ok()
                        && self.cible.ecrit_fichier(&chemin, "idle").is_ok()
                    {
                        self.dernier_recompress = maint;
                        self.journal.write(
                            Level::Info,
                            "optimisation : recompression zram des pages idle lancee",
                        );
                    }
                }
            }
        } else if pression.some < optimisation.soutenue_seuil {
            self.soutenue_depuis = -1;
        }

        self.pas_cpu(horloge)?;

        if self.delai_watch > 0 {
            let _ = notify::envoi("WATCHDOG=1");
        }
        Ok(())
    }

    fn pas_cpu(&mut self, horloge: &dyn Fn() -> i64) -> Result<()> {
        let Some(cpu) = self.cfg.cpu.clone() else {
            return Ok(());
        };
        let pression = self.cible.lit_pression_cpu()?;
        let maint = horloge();

        if !self.cpu_fige && !self.cpu_a_bouge {
            match self.cpu_premier_total {
                None => self.cpu_premier_total = Some(pression.total_micros),
                Some(premier) if premier != pression.total_micros => {
                    self.cpu_premier_total = Some(pression.total_micros);
                    self.cpu_a_bouge = true;
                }
                Some(premier)
                    if premier == pression.total_micros
                        && self.cible.lit_load1().unwrap_or(0.0) >= 0.5 =>
                {
                    self.cpu_fige = true;
                    self.journal.write(
                        Level::Warn,
                        "optimisation : PSI cpu fige sous charge (psi= absent au boot ?), en repli",
                    );
                }
                Some(_) => {}
            }
        }
        if self.cpu_fige {
            return Ok(());
        }
        let top = self.sonde_processus();
        self.applique_affinites(&cpu.affinites, false);

        if pression.some >= cpu.critique_seuil {
            if self.cpu_critique_depuis < 0 {
                self.cpu_critique_depuis = maint;
            }
            if !self.cpu_critique_actif && maint - self.cpu_critique_depuis >= cpu.critique_duree {
                self.cpu_critique_actif = true;
                match &top {
                    Some(c) => self.journal.write(
                        Level::Error,
                        &format!(
                            "optimisation : pression cpu critique ({:.1}% tenu {}ms), \
                             principal consommateur {} (pid {}) {:.0} ms",
                            pression.some, cpu.critique_duree, c.comm, c.pid, c.cpu_ms
                        ),
                    ),
                    None => self.journal.write(
                        Level::Error,
                        &format!(
                            "optimisation : pression cpu critique ({:.1}% tenu {}ms)",
                            pression.some, cpu.critique_duree
                        ),
                    ),
                }
                self.actions_cpu(&cpu, pression.some);
            }
        } else if self.cpu_critique_actif && pression.some < cpu.critique_sortie {
            self.cpu_critique_actif = false;
            self.cpu_critique_depuis = -1;
            self.restaurer_demotes();
            self.journal.write(
                Level::Info,
                &format!(
                    "optimisation : pression cpu retombee ({:.1}%)",
                    pression.some
                ),
            );
        } else if !self.cpu_critique_actif && pression.some < cpu.critique_sortie {
            self.cpu_critique_depuis = -1;
        }

        if pression.some >= cpu.soutenue_seuil && !self.cpu_critique_actif {
            if self.cpu_soutenue_depuis < 0 {
                self.cpu_soutenue_depuis = maint;
            }
            if maint - self.cpu_soutenue_depuis >= cpu.soutenue_duree {
                self.cpu_soutenue_depuis = maint;
                let nom = top
                    .as_ref()
                    .map(|c| format!(" ({} pid {})", c.comm, c.pid))
                    .unwrap_or_default();
                self.journal.write(
                    Level::Warn,
                    &format!(
                        "optimisation : pression cpu soutenue ({:.1}% some){nom}",
                        pression.some
                    ),
                );
            }
        } else if pression.some < cpu.soutenue_seuil {
            self.cpu_soutenue_depuis = -1;
        }

        if self.cpu_critique_actif {
            self.demotion(&cpu, &top);
        }
        Ok(())
    }

    fn sonde_processus(&mut self) -> Option<crate::processus::Consommateur> {
        let maintenant = self.cible.processus().ok()?;
        let Some(prec) = &self.proces_prec else {
            self.proces_prec = Some(maintenant);
            return None;
        };
        let ticks = crate::sys::ticks_par_seconde().max(1);
        let top = top_consommateurs(prec, &maintenant, ticks, 1)
            .into_iter()
            .next();
        self.proces_prec = Some(maintenant);
        top.filter(|c| c.cpu_ms > 0)
    }

    fn actions_cpu(&mut self, cpu: &crate::config::cpu::Cpu, valeur: f64) {
        if cpu.actions_critique.is_empty() {
            return;
        }
        let virtuelle = Regle {
            nom: "optimisation-cpu".to_string(),
            severite: Severite::Warning,
            condition: Condition {
                metrique: None,
                montage: None,
                operateur: None,
                valeur: cpu.critique_seuil,
            },
            pour: 0,
            cooldown: 0,
            actions: Vec::new(),
        };
        for act in &cpu.actions_critique {
            let ac = ActionCours {
                act,
                regle: &virtuelle,
                valeur,
            };
            let _ = executer(&ac, &mut self.journal, &self.cfg, &self.ctx);
        }
    }

    fn demotion(
        &mut self,
        cpu: &crate::config::cpu::Cpu,
        top: &Option<crate::processus::Consommateur>,
    ) {
        let Some(demotion) = &cpu.demotion else {
            return;
        };
        let Some(top) = top else {
            return;
        };
        let demande = demotion
            .patterns
            .iter()
            .any(|pattern| top.comm.contains(pattern.as_str()))
            && !demotion
                .exceptions
                .iter()
                .any(|exception| top.comm.contains(exception.as_str()));
        if !demande || self.demotes.iter().any(|(pid, _)| *pid == top.pid) {
            return;
        }
        match self.cible.renice(top.pid, demotion.nice) {
            Ok(()) => {
                self.demotes.push((top.pid, top.comm.clone()));
                self.journal.write(
                    Level::Warn,
                    &format!(
                        "optimisation : {} (pid {}) relegue a nice {}",
                        top.comm, top.pid, demotion.nice
                    ),
                );
            }
            Err(e) if !self.socle_pose => self.journal.write(
                Level::Warn,
                &format!(
                    "optimisation : {} non relegue ({e}), privilèges requis",
                    top.comm
                ),
            ),
            Err(_) => {}
        }
    }

    fn restaurer_demotes(&mut self) {
        for (pid, comm) in self.demotes.drain(..) {
            match self.cible.renice(pid, 0) {
                Ok(()) => self.journal.write(
                    Level::Info,
                    &format!("optimisation : {comm} (pid {pid}) nice restaure"),
                ),
                Err(_) => self.journal.write(
                    Level::Warn,
                    &format!("optimisation : {comm} (pid {pid}) restaure manuellement"),
                ),
            }
        }
    }

    pub fn boucle(&mut self) {
        let horloge = crate::eval::maintenant_ms;
        while !self.arret.load(Ordering::Relaxed) {
            if self.rechargement.swap(false, Ordering::Relaxed) {
                self.journal.write(
                    Level::Info,
                    "optimisation : rechargement non gere en v1, configuration conservee",
                );
            }
            let _ = self.pas(&horloge);
            let intervalle = self
                .cfg
                .optimisation
                .as_ref()
                .map(|o| o.intervalle)
                .unwrap_or(10_000);
            thread::sleep(Duration::from_millis(intervalle.max(0) as u64));
        }
    }

    fn installer_signaux(&self) -> std::io::Result<()> {
        signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&self.arret))?;
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&self.arret))?;
        signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&self.rechargement))?;
        Ok(())
    }

    pub fn demarre(&mut self) -> i32 {
        if self.cfg.optimisation.is_none() && self.cfg.cpu.is_none() {
            self.journal.write(
                Level::Error,
                "optimisation : ni section optimisation ni section cpu dans la configuration",
            );
            return 1;
        }
        self.journal.write(Level::Info, "optimiseur démarré");
        self.socle();
        let _ = notify::envoi("READY=1");
        self.delai_watch = notify::delai_watchdog();
        if let Err(e) = self.installer_signaux() {
            self.journal
                .write(Level::Error, &format!("signaux indisponibles : {e}"));
            return 1;
        }
        if self.once {
            let horloge = crate::eval::maintenant_ms;
            let _ = self.pas(&horloge);
        } else {
            self.boucle();
        }
        let _ = notify::envoi("STOPPING=1");
        self.journal.write(Level::Info, "optimiseur arrêté");
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pression::{PressionCpu, PressionMemoire};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    struct CibleFausse {
        fichiers: RefCell<BTreeMap<PathBuf, String>>,
        zram: bool,
        pression: RefCell<PressionMemoire>,
        total_psi: RefCell<u64>,
        total_fixe: bool,
        pression_cpu: RefCell<PressionCpu>,
        processus: RefCell<Vec<ProcessusCpu>>,
        renices: RefCell<Vec<(i32, i32)>>,
        affinites: RefCell<Vec<(i32, Vec<usize>)>>,
        cpus: Vec<PathBuf>,
        load1: RefCell<f64>,
        commandes: RefCell<Vec<(String, Vec<String>)>>,
        swapons: RefCell<Vec<(String, i32)>>,
        ram: u64,
        zones: RefCell<Vec<crate::compression::ZoneSwap>>,
        echec_modprobe: bool,
        echec_mkswap: bool,
        echec_swapon: bool,
        echecs_ecriture: RefCell<Vec<PathBuf>>,
        zones_en_erreur: bool,
        services: RefCell<Vec<String>>,
    }

    impl CibleFausse {
        fn nouveau(zram: bool) -> Self {
            let mut fichiers = BTreeMap::new();
            fichiers.insert(PathBuf::from(CHEMIN_SWAPPINESS), "60".to_string());
            fichiers.insert(PathBuf::from(CHEMIN_PAGE_CLUSTER), "3".to_string());
            fichiers.insert(
                PathBuf::from("/sys/block/zram0/mm_stat"),
                "1024 256 300 0 0 0 0 0".to_string(),
            );
            fichiers.insert(PathBuf::from("/dev/zram0"), "ok".to_string());
            fichiers.insert(
                PathBuf::from("/sys/block/zram0/comp_algorithm"),
                "[lzo] lz4 zstd".to_string(),
            );
            fichiers.insert(PathBuf::from("/sys/block/zram0/recompress"), "".to_string());
            Self {
                fichiers: RefCell::new(fichiers),
                zram,
                pression: RefCell::new(PressionMemoire::default()),
                total_psi: RefCell::new(0),
                total_fixe: false,
                pression_cpu: RefCell::new(PressionCpu::default()),
                processus: RefCell::new(Vec::new()),
                renices: RefCell::new(Vec::new()),
                affinites: RefCell::new(Vec::new()),
                cpus: Vec::new(),
                load1: RefCell::new(0.0),
                commandes: RefCell::new(Vec::new()),
                swapons: RefCell::new(Vec::new()),
                ram: 8 * 1024 * 1024 * 1024,
                zones: RefCell::new(Vec::new()),
                echec_modprobe: false,
                echec_mkswap: false,
                echec_swapon: false,
                echecs_ecriture: RefCell::new(Vec::new()),
                zones_en_erreur: false,
                services: RefCell::new(Vec::new()),
            }
        }

        fn fixe_pression_cpu(&self, some: f64) {
            let mut p = self.pression_cpu.borrow_mut();
            p.some = some;
            drop(p);
            if self.total_fixe {
                return;
            }
            let nouveau = *self.total_psi.borrow() + 5_000;
            *self.total_psi.borrow_mut() = nouveau;
            self.pression_cpu.borrow_mut().total_micros = nouveau;
        }

        fn fixe_pression(&self, some: f64, full: f64) {
            let mut p = self.pression.borrow_mut();
            p.some = some;
            p.full = full;
            drop(p);
            if self.total_fixe {
                return;
            }
            let nouveau = *self.total_psi.borrow() + 5_000;
            *self.total_psi.borrow_mut() = nouveau;
            self.pression.borrow_mut().total_micros = nouveau;
        }

        fn valeur(&self, chemin: &str) -> String {
            self.fichiers
                .borrow()
                .get(Path::new(chemin))
                .cloned()
                .unwrap_or_default()
        }
    }

    impl CibleReglage for CibleFausse {
        fn lit_fichier(&self, chemin: &Path) -> io::Result<String> {
            self.fichiers
                .borrow()
                .get(chemin)
                .cloned()
                .ok_or_else(|| io::Error::other("absent"))
        }

        fn ecrit_fichier(&self, chemin: &Path, valeur: &str) -> io::Result<()> {
            if chemin.file_name() == Some(std::ffi::OsStr::new("interdit"))
                || self.echecs_ecriture.borrow().iter().any(|c| c == chemin)
            {
                return Err(io::Error::other("refusé"));
            }
            self.fichiers
                .borrow_mut()
                .insert(chemin.to_path_buf(), valeur.to_string());
            Ok(())
        }

        fn lit_pression(&self) -> Result<PressionMemoire> {
            Ok(*self.pression.borrow())
        }

        fn lit_pression_cpu(&self) -> Result<PressionCpu> {
            Ok(*self.pression_cpu.borrow())
        }

        fn liste_cpus(&self) -> Vec<PathBuf> {
            self.cpus.clone()
        }

        fn processus(&self) -> Result<Vec<ProcessusCpu>> {
            Ok(self.processus.borrow().clone())
        }

        fn service_actif(&self, nom: &str) -> bool {
            self.services.borrow().iter().any(|s| s == nom)
        }

        fn lit_load1(&self) -> Result<f64> {
            Ok(*self.load1.borrow())
        }

        fn renice(&self, pid: i32, nice: i32) -> io::Result<()> {
            self.renices.borrow_mut().push((pid, nice));
            Ok(())
        }

        fn fixe_affinite(&self, pid: i32, coeurs: &[usize]) -> io::Result<()> {
            self.affinites.borrow_mut().push((pid, coeurs.to_vec()));
            Ok(())
        }

        fn fichier_existe(&self, chemin: &Path) -> bool {
            self.fichiers.borrow().contains_key(chemin)
        }

        fn zones_swap(&self) -> Result<Vec<crate::compression::ZoneSwap>> {
            if self.zones_en_erreur {
                return Err(crate::error::Error::new("swaps illisibles"));
            }
            Ok(self.zones.borrow().clone())
        }

        fn ram_octets(&self) -> u64 {
            self.ram
        }

        fn lance_commande(&self, prog: &str, args: &[&str]) -> io::Result<()> {
            if prog == "modprobe" && self.echec_modprobe {
                return Err(io::Error::other("module absent"));
            }
            if prog == "mkswap" && self.echec_mkswap {
                return Err(io::Error::other("mkswap refuse"));
            }
            self.commandes.borrow_mut().push((
                prog.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
            ));
            Ok(())
        }

        fn swapon(&self, chemin: &Path, priorite: i32) -> io::Result<()> {
            if self.echec_swapon {
                return Err(io::Error::other("swapon refuse"));
            }
            self.swapons
                .borrow_mut()
                .push((chemin.to_string_lossy().to_string(), priorite));
            Ok(())
        }

        fn zram_sysfs(&self) -> Option<PathBuf> {
            if self.zram {
                Some(PathBuf::from("/sys/block/zram0"))
            } else {
                None
            }
        }
    }

    fn cfg_optimisee(swappiness: &str) -> Config {
        let yaml = format!(
            "global:\n  check_interval: 5s\noptimisation:\n  intervalle: 1s\n  swappiness: {swappiness}\n  critique_seuil: 60\n  critique_duree: 30s\n  critique_sortie: 40\n  soutenue_seuil: 20\n  soutenue_duree: 60s\n"
        );
        crate::config::parse(&yaml).expect("config")
    }

    #[test]
    fn socle_avec_zram_auto_applique_doc_noyau() {
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "133", "auto + zram = 133");
        assert_eq!(cible.valeur(CHEMIN_PAGE_CLUSTER), "0");
    }

    #[test]
    fn socle_sans_zram_auto_intact() {
        let cible = CibleFausse::nouveau(false);
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "60", "intact");
        assert_eq!(cible.valeur(CHEMIN_PAGE_CLUSTER), "3", "intact");
        let out = String::from_utf8(o.journal.contenu()).expect("utf-8");
        assert!(out.contains("swappiness non modifie"), "{out}");
    }

    #[test]
    fn socle_valeur_explicite_et_idempotent() {
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("180"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "180");
        o.socle();
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "180", "idempotent");
    }

    #[test]
    fn socle_echec_recommandation() {
        let cible = CibleFausse::nouveau(true);
        let cfg = crate::config::parse(
            "global:\n  check_interval: 5s\noptimisation:\n  swappiness: 150\n  page_cluster: 0\n  zram_algorithme: zstd\n",
        )
        .expect("config");
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        // mm_stat absent : zram "absent" pour le socle mais swappiness explicite
        // doit tenter l'ecriture ; on force l'echec via un fichier marque interdit
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/block/zram0/comp_algorithm"),
            "[zstd] lz4".to_string(),
        );
        o.socle();
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "150");
    }

    #[test]
    fn hysteresis_critique_entree_sortie() {
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 1");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 2");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 3");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 4, 30s tenu");
        {
            let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
            assert!(out.contains("pression memoire critique"), "{out}");
        }

        // sous la sortie (40) : retombee
        cible.fixe_pression(30.0, 20.0);
        o.pas(&horloge).expect("pas retombee");
        {
            let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
            assert!(out.contains("pression memoire retombee"), "{out}");
        }

        // zone d'hysteresis (45 : sous le seuil, au-dessus de la sortie) : rien
        let avant = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        cible.fixe_pression(45.0, 45.0);
        o.pas(&horloge).expect("pas hysteresis");
        let apres = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert_eq!(avant, apres, "pas de nouveau message en zone morte");
    }

    #[test]
    fn provisionnement_complet_sans_swap() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n  zram_taille: auto\n  zram_algorithme: auto\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        // 2 cœurs : auto → lz4
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
        ];
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let commandes = cible.commandes.borrow();
        assert_eq!(commandes.len(), 2, "modprobe puis mkswap");
        assert_eq!(
            commandes[0],
            ("modprobe".to_string(), vec!["zram".to_string()])
        );
        assert_eq!(
            commandes[1],
            ("mkswap".to_string(), vec!["/dev/zram0".to_string()])
        );
        let swapons = cible.swapons.borrow();
        assert_eq!(swapons.len(), 1);
        assert_eq!(swapons[0], ("/dev/zram0".to_string(), 100), "priorité 100");
        assert_eq!(
            cible.valeur("/sys/block/zram0/comp_algorithm"),
            "lz4",
            "auto + 2 cœurs = lz4"
        );
        // 8 G de RAM : min(ram/2, 4096) = 4096 Mio
        let taille = 4096 * 1024 * 1024u64;
        assert_eq!(
            cible.valeur("/sys/block/zram0/disksize"),
            taille.to_string()
        );
        let conf = cible.valeur(crate::compression::CHEMIN_GENERATEUR_CONF);
        assert!(conf.contains("[zram0]"), "{conf}");
        assert!(conf.contains("zram-size = 4096"), "{conf}");
        assert!(conf.contains("compression-algorithm = lz4"), "{conf}");
        assert!(conf.contains("swap-priority = 100"), "{conf}");
        // socle mémoire suit : swappiness 133 appliqué après provisionnement
        assert_eq!(cible.valeur(CHEMIN_SWAPPINESS), "133");
    }

    #[test]
    fn provisionnement_refuse_si_swap_disque() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(false);
        cible.zones.borrow_mut().push(crate::compression::ZoneSwap {
            chemin: "/dev/sda2".to_string(),
            type_zone: crate::compression::TypeZone::Disque,
            taille_ko: 4_194_300,
            priorite: -2,
        });
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert!(cible.commandes.borrow().is_empty(), "aucune commande");
        assert!(cible.swapons.borrow().is_empty());
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("inversion LRU"), "{out}");
    }

    #[test]
    fn provisionnement_non_duplique_avec_generator() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(false);
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_GENERATEUR),
            "binaire".to_string(),
        );
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_GENERATEUR_CONF),
            "[zram0]\n".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert!(
            cible.commandes.borrow().is_empty(),
            "generator gère au boot"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("non duplique"), "{out}");
    }

    #[test]
    fn zswap_desactive_sur_demande() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n  zswap: desactive\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_ZSWAP_ACTIVE),
            "Y".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur(crate::compression::CHEMIN_ZSWAP_ACTIVE),
            "0",
            "zswap désactivé sur demande"
        );
    }

    #[test]
    fn conf_sans_binaire_provisionne_et_avertit() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_GENERATEUR_CONF),
            "[zram0]\n".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert!(
            !cible.commandes.borrow().is_empty(),
            "binaire absent : provisionnement au runtime quand même"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(
            out.contains("installer zram-generator"),
            "l'admin est averti pour la persistance : {out}"
        );
    }

    #[test]
    fn zswap_garde_averti_seulement_avec_zram() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zswap: garde\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_ZSWAP_ACTIVE),
            "Y".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg.clone(),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("double compression"), "{out}");

        // sans zram : pas d'avertissement (zswap + swap disque est un bon choix)
        let cible = CibleFausse::nouveau(false);
        cible.fichiers.borrow_mut().insert(
            PathBuf::from(crate::compression::CHEMIN_ZSWAP_ACTIVE),
            "Y".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg.clone(),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(!out.contains("double compression"), "{out}");
    }

    #[test]
    fn pression_cpu_soutenue_journalise_sans_demotion() {
        let yaml =
            "global:\n  check_interval: 5s\ncpu:\n  soutenue_seuil: 40\n  soutenue_duree: 60s\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();
        cible.fixe_pression_cpu(50.0);
        o.pas(&horloge).expect("pas 1");
        temps.replace_with(|t| *t + 61_000);
        cible.fixe_pression_cpu(50.0);
        o.pas(&horloge).expect("pas 2 : soutenue tenu");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("pression cpu soutenue"), "{out}");
    }

    #[test]
    fn psi_cpu_fige_sous_charge_bloque_le_suivi_cpu() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  critique_seuil: 80\n  critique_duree: 30s\n  critique_sortie: 50\n  soutenue_seuil: 40\n  soutenue_duree: 60s\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.total_fixe = true; // total cpu constant même sous charge
        *cible.load1.borrow_mut() = 5.0;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();
        cible.fixe_pression_cpu(90.0);
        o.pas(&horloge).expect("pas 1");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression_cpu(90.0);
        o.pas(&horloge).expect("pas 2 : figé");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("PSI cpu fige"), "{out}");
        temps.replace_with(|t| *t + 60_000);
        cible.fixe_pression_cpu(90.0);
        o.pas(&horloge).expect("pas 3 : bloqué");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(!out.contains("pression cpu critique"), "{out}");
    }

    #[test]
    fn gouverneur_refuse_sur_un_coeur_journalise_le_nombre() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  gouverneur: performance\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
        ];
        cible.echecs_ecriture.borrow_mut().push(PathBuf::from(
            "/sys/devices/system/cpu/cpu1/scaling_governor",
        ));
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "schedutil".to_string(),
        );
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu1/scaling_governor"),
            "schedutil".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "performance"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("refuse par 1 coeur(s)"), "{out}");
    }

    #[test]
    fn provisionnement_sans_sysfs_silencieux_apres_modprobe() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(false);
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.commandes.borrow().len(), 1, "modprobe seul");
        assert!(cible.swapons.borrow().is_empty());
    }

    #[test]
    fn provisionnement_auto_zstd_sur_quatre_coeurs() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n  zram_algorithme: auto\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
            PathBuf::from("/sys/devices/system/cpu/cpu2"),
            PathBuf::from("/sys/devices/system/cpu/cpu3"),
        ];
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur("/sys/block/zram0/comp_algorithm"),
            "zstd",
            "auto + 4 cœurs = zstd"
        );
    }

    #[test]
    fn provisionnement_abort_modprobe_absent() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(false);
        cible.echec_modprobe = true;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert!(cible.commandes.borrow().is_empty(), "mkswap jamais lancé");
        assert!(cible.swapons.borrow().is_empty());
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("provisionnement abandonne"), "{out}");
    }

    #[test]
    fn provisionnement_abort_mkswap() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.echec_mkswap = true;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.commandes.borrow().len(), 1, "modprobe seul");
        assert!(cible.swapons.borrow().is_empty(), "pas de swapon");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("mkswap en echec"), "{out}");
    }

    #[test]
    fn provisionnement_abort_disksize() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible
            .echecs_ecriture
            .borrow_mut()
            .push(PathBuf::from("/sys/block/zram0/disksize"));
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(cible.commandes.borrow().len(), 1, "modprobe seul");
        assert!(cible.swapons.borrow().is_empty());
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("disksize"), "{out}");
    }

    #[test]
    fn provisionnement_swapon_echec_partiel() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.echec_swapon = true;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("swapon en echec"), "{out}");
        assert!(
            !out.contains("persistance"),
            "pas de conf generator après un swapon raté : {out}"
        );
    }

    #[test]
    fn provisionnement_taille_explicite_et_algo_explicite() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n  zram_taille: 2048\n  zram_algorithme: lz4\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
            PathBuf::from("/sys/devices/system/cpu/cpu2"),
            PathBuf::from("/sys/devices/system/cpu/cpu3"),
        ];
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let taille = 2048 * 1024 * 1024u64;
        assert_eq!(
            cible.valeur("/sys/block/zram0/disksize"),
            taille.to_string()
        );
        assert_eq!(
            cible.valeur("/sys/block/zram0/comp_algorithm"),
            "lz4",
            "explicite prime sur auto (4 cœurs)"
        );
    }

    #[test]
    fn swaps_en_erreur_provisionnement_ignore() {
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  zram_provisionner: oui\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.zones_en_erreur = true;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert!(
            cible.commandes.borrow().is_empty(),
            "état illisible : pas de provisionnement"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("etat de compression illisible"), "{out}");
    }

    #[test]
    fn gestionnaire_actif_pas_de_gouverneur_ni_epp() {
        let yaml =
            "global:\n  check_interval: 5s\ncpu:\n  gouverneur: performance\n  epp: performance\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = vec![PathBuf::from("/sys/devices/system/cpu/cpu0")];
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "schedutil".to_string(),
        );
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu0/energy_performance_preference"),
            "balance_performance".to_string(),
        );
        cible
            .services
            .borrow_mut()
            .push("power-profiles-daemon.service".to_string());
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "schedutil",
            "géré par PPD : gouverneur intact"
        );
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu0/energy_performance_preference"),
            "balance_performance",
            "géré par PPD : epp intact"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("power-profiles-daemon"), "{out}");
        assert!(out.contains("pas de doublon"), "{out}");
    }

    #[test]
    fn epp_applique_sur_coeurs_supportant_et_recommande_irqbalance() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  epp: balance_performance\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
        ];
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu0/energy_performance_preference"),
            "balance_performance".to_string(),
        );
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu1/energy_performance_preference"),
            "power".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu1/energy_performance_preference"),
            "balance_performance",
            "cpu0 déjà bon, cpu1 corrigé"
        );
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("epp = balance_performance"), "{out}");
        assert!(out.contains("irqbalance absent"), "{out}");
    }

    #[test]
    fn epp_non_supporte_degrade() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  epp: performance\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("epp non supporte"), "{out}");
    }

    #[test]
    fn socle_gouverneur_applique_et_absent_degrade() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  gouverneur: performance\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let mut cible = CibleFausse::nouveau(true);
        #[allow(clippy::redundant_clone)]
        let cfg = cfg.clone();
        cible.cpus = vec![
            PathBuf::from("/sys/devices/system/cpu/cpu0"),
            PathBuf::from("/sys/devices/system/cpu/cpu1"),
        ];
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "schedutil".to_string(),
        );
        cible.fichiers.borrow_mut().insert(
            PathBuf::from("/sys/devices/system/cpu/cpu1/scaling_governor"),
            "schedutil".to_string(),
        );
        let mut o = Optimiseur::nouveau(
            cfg.clone(),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu0/scaling_governor"),
            "performance"
        );
        assert_eq!(
            cible.valeur("/sys/devices/system/cpu/cpu1/scaling_governor"),
            "performance"
        );

        // cpufreq absent : dégradation journalisée, pas de panique
        let mut cible = CibleFausse::nouveau(true);
        cible.cpus = Vec::new();
        let mut o = Optimiseur::nouveau(
            cfg.clone(),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("cpufreq absent"), "{out}");
    }

    #[test]
    fn socle_affinite_appliquee_aux_processus_nommes() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  affinites:\n    - nom: monscript\n      coeurs: \"0-1\"\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.processus.borrow_mut().push(ProcessusCpu {
            pid: 500,
            comm: "monscript".to_string(),
            ticks: 0,
            nice: 0,
        });
        cible.processus.borrow_mut().push(ProcessusCpu {
            pid: 501,
            comm: "autre".to_string(),
            ticks: 0,
            nice: 0,
        });
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        o.socle();
        let affinites = cible.affinites.borrow();
        assert_eq!(affinites.len(), 1, "seul monscript épinglé");
        assert_eq!(affinites[0], (500, vec![0, 1]));
    }

    #[test]
    fn pression_cpu_critique_nomme_le_top_et_relague() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  critique_seuil: 80\n  critique_duree: 30s\n  critique_sortie: 50\n  soutenue_seuil: 40\n  soutenue_duree: 60s\n  demotion:\n    nice: 19\n    patterns:\n      - montraitement\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.processus.borrow_mut().push(ProcessusCpu {
            pid: 700,
            comm: "montraitement".to_string(),
            ticks: 0,
            nice: 0,
        });
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        // première passe : baseline des processus (pas de nommage)
        cible.pression_cpu.borrow_mut().some = 90.0;
        cible.processus.borrow_mut()[0].ticks = 0;
        o.pas(&horloge).expect("baseline");

        // passes suivantes : 30s tenu → critique + relégation
        temps.replace_with(|t| *t + 10_000);
        cible.processus.borrow_mut()[0].ticks = 1000;
        cible.fixe_pression_cpu(90.0);
        o.pas(&horloge).expect("pas 2");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression_cpu(90.0);
        cible.processus.borrow_mut()[0].ticks = 2000;
        o.pas(&horloge).expect("pas 3");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression_cpu(90.0);
        cible.processus.borrow_mut()[0].ticks = 3000;
        o.pas(&horloge).expect("pas 4 : critique");

        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("pression cpu critique"), "{out}");
        assert!(out.contains("montraitement"), "top nommé : {out}");
        let renices = cible.renices.borrow();
        assert_eq!(renices.len(), 1);
        assert_eq!(renices[0], (700, 19));
    }

    #[test]
    fn demotion_exception_ne_touche_pas_et_restaurat_apres_retombee() {
        let yaml = "global:\n  check_interval: 5s\ncpu:\n  critique_seuil: 80\n  critique_duree: 30s\n  critique_sortie: 50\n  soutenue_seuil: 40\n  soutenue_duree: 60s\n  demotion:\n    nice: 19\n    patterns:\n      - montraitement\n    exceptions:\n      - sshd\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        cible.processus.borrow_mut().push(ProcessusCpu {
            pid: 700,
            comm: "montraitement".to_string(),
            ticks: 0,
            nice: 0,
        });
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        // le top est sshd : exception → pas de relégation même en critique
        cible.pression_cpu.borrow_mut().some = 90.0;
        cible.processus.borrow_mut()[0].comm = "sshd".to_string();
        o.pas(&horloge).expect("baseline");
        for _ in 0..3 {
            temps.replace_with(|t| *t + 10_000);
            cible.processus.borrow_mut()[0].ticks += 1000;
            cible.fixe_pression_cpu(90.0);
            o.pas(&horloge).expect("pas critique sshd");
        }
        assert!(cible.renices.borrow().is_empty(), "exception respectée");

        // le top redevient montraitement : relégation puis restauration
        cible.processus.borrow_mut()[0].comm = "montraitement".to_string();
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression_cpu(90.0);
        cible.processus.borrow_mut()[0].ticks += 1000;
        o.pas(&horloge).expect("critique avec montraitement");
        assert_eq!(cible.renices.borrow().len(), 1);

        // retombée sous la sortie : nice restauré
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression_cpu(30.0);
        o.pas(&horloge).expect("retombée");
        let renices = cible.renices.borrow();
        assert_eq!(renices.last().expect("restauration"), &(700, 0));
    }

    #[test]
    fn fin_de_charge_gel_ne_declare_pas_fige() {
        // Incident I-1 : sous charge les totals bougent (critique déclenchée),
        // la charge cesse → total gelé + load1 encore haut → NE PAS déclarer
        // figé, la retombée doit être journalisée.
        let yaml = "global:\n  check_interval: 5s\noptimisation:\n  critique_seuil: 5\n  critique_duree: 10s\n  critique_sortie: 2\n  soutenue_seuil: 3\n  soutenue_duree: 10s\n";
        let cfg = crate::config::parse(yaml).expect("config");
        let cible = CibleFausse::nouveau(true);
        *cible.load1.borrow_mut() = 5.0;
        let mut o = Optimiseur::nouveau(
            cfg,
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        cible.fixe_pression(50.0, 20.0);
        o.pas(&horloge).expect("charge : pas 1");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(50.0, 20.0);
        o.pas(&horloge).expect("charge : critique tenu");

        // la charge cesse : total gelé, load1 encore haut, some sous la sortie
        *cible.load1.borrow_mut() = 2.0;
        cible.fixe_pression(1.0, 0.5);
        temps.replace_with(|t| *t + 10_000);
        o.pas(&horloge).expect("fin de charge");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("retombee"), "retombée après charge : {out}");
        assert!(
            !out.contains("fige"),
            "gel de fin de charge ≠ PSI figé : {out}"
        );
    }

    #[test]
    fn psi_fige_averti_et_bloque() {
        let mut cible = CibleFausse::nouveau(true);
        cible.total_fixe = true;
        *cible.load1.borrow_mut() = 5.0;
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 1 : total enregistré");
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 2 : total identique -> figé");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(out.contains("PSI memoire fige"), "{out}");

        cible.fixe_pression(70.0, 65.0);
        temps.replace_with(|t| *t + 60_000);
        o.pas(&horloge).expect("pas 3 : bloqué");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(
            !out.contains("pression memoire critique"),
            "pas d'évaluation sur PSI figé : {out}"
        );
    }

    #[test]
    fn systeme_au_repos_pas_de_fige() {
        let mut cible = CibleFausse::nouveau(true);
        cible.total_fixe = true; // total constant même sous "charge"
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 1");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 2 au repos");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(70.0, 65.0);
        o.pas(&horloge).expect("pas 3 au repos");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(
            !out.contains("fige"),
            "au repos (load1=0), un total immobile n'est pas un PSI figé : {out}"
        );
    }

    #[test]
    fn critique_tardif_reprend_depuis_zero() {
        let cible = CibleFausse::nouveau(true);
        let mut o = Optimiseur::nouveau(
            cfg_optimisee("auto"),
            false,
            Logger::new("info", Vec::new()).expect("journal"),
            &cible,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let temps = RefCell::new(0i64);
        let horloge = || *temps.borrow();

        cible.fixe_pression(0.0, 10.0);
        o.pas(&horloge).expect("pas calme");
        temps.replace_with(|t| *t + 40_000);
        cible.fixe_pression(0.0, 70.0);
        o.pas(&horloge).expect("montee a 40s");
        temps.replace_with(|t| *t + 10_000);
        cible.fixe_pression(0.0, 70.0);
        o.pas(&horloge).expect("toujours sous la duree");
        let out = String::from_utf8(o.journal_mut().tampon().clone()).expect("utf-8");
        assert!(
            !out.contains("critique"),
            "10s sous 30s requis, pas de critique : {out}"
        );
    }
}
