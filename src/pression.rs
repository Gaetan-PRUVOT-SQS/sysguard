use std::path::Path;

use crate::error::{Error, Result};
use crate::file;

/// Pression mémoire : lignes `some` et `full` de /proc/pressure/memory.
#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct PressionMemoire {
    pub some: f64,
    pub full: f64,
    pub total_micros: u64,
}

/// Pression CPU : ligne `some` de /proc/pressure/cpu (full absent au
/// niveau système depuis le noyau 5.13).
#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct PressionCpu {
    pub some: f64,
    pub total_micros: u64,
}

pub fn lire_pression(chemin: &Path) -> Result<PressionMemoire> {
    let mut tampon = [0u8; 1024];
    let texte = file::read_proc(chemin, &mut tampon)?;
    parse_pression_memoire(texte)
}

pub fn lire_pression_cpu(chemin: &Path) -> Result<PressionCpu> {
    let mut tampon = [0u8; 512];
    let texte = file::read_proc(chemin, &mut tampon)?;
    parse_pression_cpu(texte)
}

pub fn parse_pression_memoire(texte: &str) -> Result<PressionMemoire> {
    let mut p = PressionMemoire::default();
    let mut vu_some = false;
    for ligne in texte.lines() {
        let Some((genre, mesures)) = ligne.split_once(' ') else {
            continue;
        };
        let Some(avg10) = mesures
            .split_whitespace()
            .find_map(|c| c.strip_prefix("avg10="))
            .and_then(|v| v.parse::<f64>().ok())
        else {
            continue;
        };
        if genre == "some"
            && let Some(total) = mesures
                .split_whitespace()
                .find_map(|c| c.strip_prefix("total="))
                .and_then(|v| v.parse::<u64>().ok())
        {
            p.total_micros = total;
        }
        match genre {
            "some" => {
                p.some = avg10;
                vu_some = true;
            }
            "full" => p.full = avg10,
            _ => {}
        }
    }
    if !vu_some {
        return Err(Error::new("psi mémoire illisible"));
    }
    Ok(p)
}

pub fn parse_pression_cpu(texte: &str) -> Result<PressionCpu> {
    let mut p = PressionCpu::default();
    let mut vu_some = false;
    for ligne in texte.lines() {
        let Some((genre, mesures)) = ligne.split_once(' ') else {
            continue;
        };
        let Some(avg10) = mesures
            .split_whitespace()
            .find_map(|c| c.strip_prefix("avg10="))
            .and_then(|v| v.parse::<f64>().ok())
        else {
            continue;
        };
        if genre == "some"
            && let Some(total) = mesures
                .split_whitespace()
                .find_map(|c| c.strip_prefix("total="))
                .and_then(|v| v.parse::<u64>().ok())
        {
            p.total_micros = total;
        }
        if genre == "some" {
            p.some = avg10;
            vu_some = true;
        }
    }
    if !vu_some {
        return Err(Error::new("psi cpu illisible"));
    }
    Ok(p)
}

#[derive(Debug, PartialEq)]
pub struct SwapZone {
    pub chemin: String,
    pub taille_ko: u64,
    pub utilise_ko: u64,
    pub priorite: i32,
}

pub fn parse_swaps(texte: &str) -> Vec<SwapZone> {
    texte
        .lines()
        .skip(1)
        .filter_map(|ligne| {
            let champs: Vec<&str> = ligne.split_whitespace().collect();
            if champs.len() < 5 {
                return None;
            }
            Some(SwapZone {
                chemin: champs[0].to_string(),
                taille_ko: champs[2].parse().ok()?,
                utilise_ko: champs[3].parse().ok()?,
                priorite: champs[4].parse().ok()?,
            })
        })
        .collect()
}

pub fn lire_swaps(chemin: &Path) -> Result<Vec<SwapZone>> {
    let mut tampon = [0u8; 4096];
    let texte = file::read_proc(chemin, &mut tampon)?;
    Ok(parse_swaps(texte))
}

#[derive(Debug, PartialEq)]
pub struct ZramSante {
    pub orig_octets: u64,
    pub compr_octets: u64,
    pub memoire_utilisee_octets: u64,
    pub taille_ko: u64,
    pub algorithme: String,
}

impl ZramSante {
    pub fn ratio(&self) -> f64 {
        if self.compr_octets == 0 {
            return 0.0;
        }
        self.orig_octets as f64 / self.compr_octets as f64
    }
}

pub fn lire_zram(sysfs: &Path) -> Result<ZramSante> {
    let mm_stat = Path::new(sysfs).join("mm_stat");
    let mut tampon = [0u8; 512];
    let texte = file::read_proc(&mm_stat, &mut tampon)?;
    let champs: Vec<&str> = texte.split_whitespace().collect();
    if champs.len() < 3 {
        return Err(Error::new("mm_stat illisible"));
    }
    let orig: u64 = champs[0]
        .parse()
        .map_err(|_| Error::new("mm_stat illisible"))?;
    let compr: u64 = champs[1]
        .parse()
        .map_err(|_| Error::new("mm_stat illisible"))?;
    let utilise: u64 = champs[2]
        .parse()
        .map_err(|_| Error::new("mm_stat illisible"))?;
    let taille_ko = utilise.div_ceil(1024);
    let algo_chemin = Path::new(sysfs).join("comp_algorithm");
    let mut tampon_algo = [0u8; 256];
    let algo = file::read_proc(&algo_chemin, &mut tampon_algo)?;
    Ok(ZramSante {
        orig_octets: orig,
        compr_octets: compr,
        memoire_utilisee_octets: utilise,
        taille_ko,
        algorithme: algo_actif(algo),
    })
}

pub fn algo_actif(liste: &str) -> String {
    liste
        .split_whitespace()
        .find(|morceau| morceau.starts_with('['))
        .map(|morceau| morceau.trim_matches(['[', ']']).to_string())
        .unwrap_or_else(|| liste.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(nom: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(nom),
        )
        .expect("fixture lue")
    }

    #[test]
    fn pression_memoire_complete() {
        let p = parse_pression_memoire(&fixture("psi-memory")).expect("valide");
        assert_eq!(p.some, 12.34);
        assert_eq!(p.full, 3.21);
        assert_eq!(p.total_micros, 123_456);
    }

    #[test]
    fn pression_cpu_separee() {
        let texte = "some avg10=4.20 avg60=1.00 avg300=0.30 total=98765\n";
        let p = parse_pression_cpu(texte).expect("valide");
        assert_eq!(p.some, 4.2);
        assert_eq!(p.total_micros, 98_765);
        assert!(
            parse_pression_cpu("full avg10=1.00\n").is_err(),
            "cpu sans full"
        );
    }

    #[test]
    fn pression_memoire_sans_full() {
        let p = parse_pression_memoire("some avg10=1.50 avg60=0.00 avg300=0.00 total=0\n")
            .expect("valide");
        assert_eq!(p.some, 1.5);
        assert_eq!(p.full, 0.0);
        assert_eq!(p.total_micros, 0);
    }

    #[test]
    fn pression_illisible() {
        assert!(parse_pression_memoire("").is_err());
        assert!(parse_pression_memoire("nimportequoi\n").is_err());
        assert!(parse_pression_memoire("some avg10=abc\n").is_err());
        assert!(parse_pression_cpu("").is_err());
    }

    #[test]
    fn swaps_parsees() {
        let zones = parse_swaps(&fixture("swaps"));
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].chemin, "/dev/zram0");
        assert_eq!(zones[0].taille_ko, 2_097_148);
        assert_eq!(zones[0].utilise_ko, 1024);
        assert_eq!(zones[0].priorite, 100);
        assert_eq!(zones[1].priorite, -2);
        assert!(parse_swaps("").is_empty());
        assert!(parse_swaps("Filename Type Size Used Priority\n").is_empty());
    }

    #[test]
    fn algo_actif_extraire() {
        assert_eq!(algo_actif("[zstd] lzo lz4 deflate-string"), "zstd");
        assert_eq!(algo_actif("lzo [lz4] zstd"), "lz4");
        assert_eq!(algo_actif("zstd"), "zstd");
    }
}
