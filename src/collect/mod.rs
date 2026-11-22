pub mod disk;
pub mod memory;

use std::io;
use std::path::PathBuf;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::file;

#[derive(Debug, PartialEq)]
pub struct Disque {
    pub montage: String,
    pub usage: f64,
    pub libre: f64,
    pub inodes: f64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Mesures {
    pub cpu_usage: f64,
    pub mem_usage: f64,
    pub mem_disponible: f64,
    pub swap_usage: f64,
    pub load1: f64,
    pub disques: Vec<Disque>,
}

#[derive(Debug, PartialEq)]
pub struct StatfsInfo {
    pub bsize: u64,
    pub blocks: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
}

pub trait SourceStatfs {
    fn statfs(&self, montage: &str) -> io::Result<StatfsInfo>;
}

#[derive(Debug)]
pub struct Chemins {
    pub stat: PathBuf,
    pub meminfo: PathBuf,
    pub loadavg: PathBuf,
}

impl Default for Chemins {
    fn default() -> Self {
        Self {
            stat: PathBuf::from("/proc/stat"),
            meminfo: PathBuf::from("/proc/meminfo"),
            loadavg: PathBuf::from("/proc/loadavg"),
        }
    }
}

pub struct Collecteur<'a> {
    statfs: &'a dyn SourceStatfs,
    pub chemins: Chemins,
    pub mesures: Mesures,
    prec_total: u64,
    prec_idle: u64,
    cpu_pret: bool,
}

impl<'a> Collecteur<'a> {
    pub fn nouveau(statfs: &'a dyn SourceStatfs) -> Self {
        Self {
            statfs,
            chemins: Chemins::default(),
            mesures: Mesures::default(),
            prec_total: 0,
            prec_idle: 0,
            cpu_pret: false,
        }
    }

    pub fn tick(&mut self, cfg: &Config) -> Result<()> {
        self.tick_cpu()?;
        memory::collecte(self)?;
        disk::collecte(self, cfg)?;
        Ok(())
    }

    fn tick_cpu(&mut self) -> Result<()> {
        let mut tampon = [0u8; 4096];
        let texte = file::read_proc(&self.chemins.stat, &mut tampon)?;
        let (total, idle) = cpu_ligne(texte).ok_or_else(|| Error::new("ligne cpu absente"))?;
        if !self.cpu_pret {
            self.cpu_pret = true;
            self.prec_total = total;
            self.prec_idle = idle;
            self.mesures.cpu_usage = 0.0;
            return Ok(());
        }
        let dt = total.wrapping_sub(self.prec_total) as i64;
        let di = idle.wrapping_sub(self.prec_idle) as i64;
        self.prec_total = total;
        self.prec_idle = idle;
        self.mesures.cpu_usage = if dt <= 0 {
            0.0
        } else {
            (dt - di) as f64 / dt as f64 * 100.0
        };
        Ok(())
    }
}

fn cpu_ligne(texte: &str) -> Option<(u64, u64)> {
    let pos = texte.find("cpu ")?;
    let bytes = texte.as_bytes();
    let mut p = pos + 4;
    let mut champs = [0u64; 8];
    for champ in &mut champs {
        *champ = nombre_u64(bytes, &mut p);
    }
    let total = champs.iter().fold(0u64, |acc, v| acc.wrapping_add(*v));
    let idle = champs[3].wrapping_add(champs[4]);
    Some((total, idle))
}

pub(crate) fn nombre_u64(bytes: &[u8], pos: &mut usize) -> u64 {
    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    let mut valeur = 0u64;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        valeur = valeur
            .saturating_mul(10)
            .saturating_add(u64::from(bytes[*pos] - b'0'));
        *pos += 1;
    }
    valeur
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TypeAction, parse};
    use std::path::Path;

    struct FausseSource {
        ko: bool,
        zero: bool,
    }

    impl SourceStatfs for FausseSource {
        fn statfs(&self, montage: &str) -> io::Result<StatfsInfo> {
            if self.ko || montage == "/absente" {
                return Err(io::Error::other("statfs refusé"));
            }
            if self.zero {
                return Ok(StatfsInfo {
                    bsize: 0,
                    blocks: 0,
                    bavail: 0,
                    files: 0,
                    ffree: 0,
                });
            }
            Ok(StatfsInfo {
                bsize: 4096,
                blocks: 100,
                bavail: 25,
                files: 10,
                ffree: 2,
            })
        }
    }

    fn fixture_proc(nom: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("proc")
            .join(nom)
    }

    fn cfg_disques() -> Config {
        parse(
            "rules:\n  - name: d1\n    condition:\n      metric: disk.usage\n      mount: \"/\"\n      operator: \">\"\n      value: 90\n    actions:\n      - type: log\n  - name: d2\n    condition:\n      metric: disk.free\n      mount: \"/\"\n      operator: \"<\"\n      value: 1\n    actions:\n      - type: log\n",
        )
        .expect("config disques")
    }

    #[test]
    fn premier_et_deuxieme_tick() {
        let cfg = cfg_disques();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("premier tick");
        assert_eq!(c.mesures.cpu_usage, 0.0);
        assert_eq!(c.mesures.mem_usage, 50.0);
        assert_eq!(c.mesures.mem_disponible, 8000.0 / 1024.0);
        assert_eq!(c.mesures.swap_usage, 75.0);
        assert_eq!(c.mesures.load1, 0.42);

        c.chemins.stat = fixture_proc("stat-t2");
        c.tick(&cfg).expect("second tick");
        assert_eq!(c.mesures.cpu_usage, 50.0);
    }

    #[test]
    fn saturation_full_rend_cent() {
        let cfg = Config::default();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick 1");
        c.chemins.stat = fixture_proc("stat-sature");
        c.tick(&cfg).expect("tick 2");
        assert_eq!(c.mesures.cpu_usage, 100.0, "idle delta nul = saturation");
    }

    #[test]
    fn recul_ramene_a_zero() {
        let cfg = cfg_disques();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t2");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick 1");
        c.chemins.stat = fixture_proc("stat-t1");
        c.tick(&cfg).expect("tick 2");
        assert_eq!(c.mesures.cpu_usage, 0.0);
    }

    #[test]
    fn swap_absent_vaut_zero() {
        let cfg = Config::default();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo-sans-swap");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick");
        assert_eq!(c.mesures.swap_usage, 0.0);
    }

    #[test]
    fn erreurs_proc() {
        let cfg = Config::default();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");

        c.chemins.stat = PathBuf::from("/inexistant-stat");
        assert!(c.tick(&cfg).is_err(), "stat illisible");
        c.chemins.stat = fixture_proc("stat-t1");

        for nom in [
            "meminfo-manque",
            "meminfo-zero",
            "meminfo-sans-swapfree",
            "meminfo-abc",
        ] {
            c.chemins.meminfo = fixture_proc(nom);
            assert!(c.tick(&cfg).is_err(), "{nom} : erreur attendue");
        }
        c.chemins.meminfo = fixture_proc("meminfo-espaces");
        c.tick(&cfg).expect("meminfo avec tabulations");

        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg-abc");
        assert!(c.tick(&cfg).is_err(), "loadavg non numérique");
    }

    #[test]
    fn disques_dedup_et_calculs() {
        let cfg = cfg_disques();
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick disques");
        assert_eq!(c.mesures.disques.len(), 1, "montage dédupliqué");
        let d = &c.mesures.disques[0];
        assert_eq!(d.montage, "/");
        assert_eq!(d.usage, 75.0);
        assert_eq!(d.libre, 102_400.0);
        assert_eq!(d.inodes, 80.0);
    }

    #[test]
    fn statfs_en_echec_omis_silencieusement() {
        let cfg = cfg_disques();
        let source = FausseSource {
            ko: true,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick toléré");
        assert!(c.mesures.disques.is_empty(), "montage omis");
    }

    #[test]
    fn statfs_zeros_toleres() {
        let cfg = cfg_disques();
        let source = FausseSource {
            ko: false,
            zero: true,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick zéros");
        let d = &c.mesures.disques[0];
        assert_eq!(d.usage, 0.0);
        assert_eq!(d.libre, 0.0);
        assert_eq!(d.inodes, 0.0);
    }

    #[test]
    fn montage_absente_omise() {
        let cfg = parse(
            "rules:\n  - name: d\n    condition:\n      metric: disk.usage\n      mount: \"/absente\"\n      operator: \">\"\n      value: 90\n    actions:\n      - type: log\n",
        )
        .expect("config");
        let source = FausseSource {
            ko: false,
            zero: false,
        };
        let mut c = Collecteur::nouveau(&source);
        c.chemins.stat = fixture_proc("stat-t1");
        c.chemins.meminfo = fixture_proc("meminfo");
        c.chemins.loadavg = fixture_proc("loadavg");
        c.tick(&cfg).expect("tick");
        assert!(c.mesures.disques.is_empty());
    }

    #[test]
    fn tick_reel_linux() {
        let cfg = Config::default();
        let source = crate::sys::SourceReelle;
        let mut c = Collecteur::nouveau(&source);
        c.tick(&cfg).expect("tick réel");
        assert!(c.mesures.cpu_usage >= 0.0 && c.mesures.cpu_usage <= 100.0);
        assert!(c.mesures.mem_usage >= 0.0 && c.mesures.mem_usage <= 100.0);
        assert!(c.mesures.mem_disponible > 0.0);
        assert!(c.mesures.load1 >= 0.0);
        assert!(c.mesures.disques.is_empty(), "aucune règle disque");
    }

    #[test]
    fn cpu_ligne_limites() {
        assert!(cpu_ligne("rien ici").is_none());
        let (total, idle) = cpu_ligne("cpu  1 2 3 4 5 6 7 8\n").expect("valide");
        assert_eq!(total, 36);
        assert_eq!(idle, 9);
        let (total, idle) = cpu_ligne("cpu  1 2\n").expect("champs manquants à zéro");
        assert_eq!(total, 3);
        assert_eq!(idle, 0);
    }

    #[test]
    fn type_action_present_dans_config_disques() {
        let cfg = cfg_disques();
        assert_eq!(cfg.regles[0].actions[0].kind, TypeAction::Log);
    }

    #[test]
    fn nombre_u64_comportement() {
        let mut pos = 0;
        assert_eq!(nombre_u64(b"42 7", &mut pos), 42);
        assert_eq!(pos, 2);
        let mut pos = 0;
        assert_eq!(nombre_u64(b" 7x", &mut pos), 7);
        assert_eq!(pos, 2);
        let mut pos = 0;
        assert_eq!(nombre_u64(b"x", &mut pos), 0);
        assert_eq!(pos, 0);
    }
}
