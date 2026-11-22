use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config;
use crate::daemon::Daemon;
use crate::duration::format_duration;
use crate::log::{Level, Logger};
use crate::optimisation::Optimiseur;
use crate::sys::SourceReelle;

pub const VERSION: &str = "1.4.0";

pub struct Options {
    pub commande: Option<String>,
    pub chemin: PathBuf,
    pub once: bool,
}

pub fn afficher_usage<W: Write>(mut sortie: W) {
    let _ = writeln!(
        sortie,
        "usage : sysguard [--config FICHIER] {{run|check|dry-run|version}}"
    );
    let _ = writeln!(sortie, "  run       lance le daemon (par défaut)");
    let _ = writeln!(sortie, "  optimise  lance l'optimiseur (PSI, zram, tuning)");
    let _ = writeln!(sortie, "  check     valide la configuration et sort");
    let _ = writeln!(
        sortie,
        "  dry-run   évalue les règles sans exécuter les actions"
    );
    let _ = writeln!(sortie, "  version   affiche la version");
}

pub fn parse_options(args: &[String]) -> Option<Options> {
    let mut opt = Options {
        commande: None,
        chemin: PathBuf::from("/etc/sysguard/sysguard.yaml"),
        once: false,
    };
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        if let Some(chemin) = arg.strip_prefix("--config=") {
            opt.chemin = PathBuf::from(chemin);
        } else if arg == "--config" {
            if i + 1 >= args.len() {
                return None;
            }
            opt.chemin = PathBuf::from(&args[i + 1]);
            i += 1;
        } else if arg == "--once" {
            opt.once = true;
        } else if opt.commande.is_none() {
            opt.commande = Some(arg.to_string());
        } else {
            return None;
        }
        i += 1;
    }
    Some(opt)
}

pub fn sysguard_main(args: &[String]) -> i32 {
    let Some(opt) = parse_options(args) else {
        return 2;
    };
    let commande = opt.commande.clone().unwrap_or_else(|| "run".to_string());
    executer_commande(&commande, &opt)
}

fn executer_commande(commande: &str, opt: &Options) -> i32 {
    match commande {
        "version" => {
            println!("sysguard {VERSION}");
            0
        }
        "help" | "-h" | "--help" => {
            afficher_usage(std::io::stdout().lock());
            0
        }
        "check" => traiter_check(opt),
        "run" | "dry-run" => lancer_daemon(opt),
        "optimise" => lancer_optimiseur(opt),
        _ => {
            eprintln!("commande inconnue : {commande}");
            afficher_usage(std::io::stderr().lock());
            2
        }
    }
}

fn traiter_check(opt: &Options) -> i32 {
    match config::charge(&opt.chemin) {
        Err(e) => {
            eprintln!("configuration invalide : {e}");
            1
        }
        Ok(cfg) => {
            println!(
                "configuration valide : {} regles, intervalle {}",
                cfg.regles.len(),
                format_duration(cfg.intervalle)
            );
            0
        }
    }
}

fn lancer_daemon(opt: &Options) -> i32 {
    let journal = match Logger::new("info", std::io::stdout()) {
        Ok(j) => j,
        Err(_) => return 1,
    };
    let source = SourceReelle;
    let mut daemon = Daemon::nouveau(
        opt.chemin.clone(),
        opt.once,
        commande_dry_run(opt),
        journal,
        &source,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    daemon.demarre()
}

fn commande_dry_run(opt: &Options) -> bool {
    opt.commande.as_deref() == Some("dry-run")
}

fn lancer_optimiseur(opt: &Options) -> i32 {
    let mut journal = match Logger::new("info", std::io::stdout()) {
        Ok(j) => j,
        Err(_) => return 1,
    };
    let cfg = match config::charge(&opt.chemin) {
        Ok(c) => c,
        Err(e) => {
            journal.write(Level::Error, &format!("configuration invalide : {e}"));
            return 1;
        }
    };
    if cfg.optimisation.is_none() && cfg.cpu.is_none() {
        journal.write(
            Level::Error,
            "optimisation : ni section optimisation ni section cpu",
        );
        return 1;
    }
    let mut optimiseur = Optimiseur::nouveau(
        cfg,
        opt.once,
        journal,
        &crate::optimisation::CibleReelle,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    optimiseur.demarre()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(liste: &[&str]) -> Vec<String> {
        std::iter::once("sysguard")
            .chain(liste.iter().copied())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn options_par_defaut() {
        let opt = parse_options(&args(&["run"])).expect("valide");
        assert_eq!(opt.commande.as_deref(), Some("run"));
        assert_eq!(opt.chemin, PathBuf::from("/etc/sysguard/sysguard.yaml"));
        assert!(!opt.once);
    }

    #[test]
    fn options_config_formes() {
        let opt = parse_options(&args(&["--config", "/tmp/a.yaml", "check"])).expect("valide");
        assert_eq!(opt.chemin, PathBuf::from("/tmp/a.yaml"));
        let opt = parse_options(&args(&["--config=/tmp/b.yaml", "check"])).expect("valide");
        assert_eq!(opt.chemin, PathBuf::from("/tmp/b.yaml"));
        let opt = parse_options(&args(&["check", "--once"])).expect("valide");
        assert!(opt.once);
        let opt = parse_options(&args(&["--once", "run", "--config=x"])).expect("valide");
        assert_eq!(opt.commande.as_deref(), Some("run"));
    }

    #[test]
    fn options_erreurs() {
        assert!(
            parse_options(&args(&["--config"])).is_none(),
            "config sans valeur"
        );
        assert!(
            parse_options(&args(&["check", "version"])).is_none(),
            "deux commandes"
        );
        assert!(
            parse_options(&args(&["run", "extra"])).is_none(),
            "argument en trop"
        );
    }

    #[test]
    fn bare_option_config_est_erreur_de_parse() {
        assert_eq!(sysguard_main(&args(&["--config"])), 2);
    }

    #[test]
    fn options_seules_impliquent_run() {
        let valide = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/daemon-once.yaml")
            .to_string_lossy()
            .to_string();
        let opt = parse_options(&args(&["--config", &valide, "--once"])).expect("valide");
        assert_eq!(opt.commande, None, "aucune commande explicite");
        assert_eq!(executer_commande("run", &opt), 0);
    }
}
