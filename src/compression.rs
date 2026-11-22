use std::path::Path;

use crate::error::Result;
use crate::optimisation::CibleReglage;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TypeZone {
    Zram,
    Disque,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ZoneSwap {
    pub chemin: String,
    pub type_zone: TypeZone,
    pub taille_ko: u64,
    pub priorite: i32,
}

#[derive(Debug, Default, PartialEq)]
pub struct EtatCompression {
    pub zones: Vec<ZoneSwap>,
    pub zram_presents: bool,
    pub generator_installe: bool,
    pub generator_confige: bool,
    pub zswap_actif: Option<bool>,
}

pub const CHEMIN_GENERATEUR: &str = "/usr/lib/systemd/system-generators/zram-generator";
pub const CHEMIN_GENERATEUR_CONF: &str = "/etc/systemd/zram-generator.conf";
pub const CHEMIN_ZSWAP_ACTIVE: &str = "/sys/module/zswap/parameters/enabled";

pub fn type_zone(chemin: &str) -> TypeZone {
    if chemin.starts_with("/dev/zram") {
        TypeZone::Zram
    } else {
        TypeZone::Disque
    }
}

pub fn detecte(cible: &dyn CibleReglage) -> Result<EtatCompression> {
    let mut etat = EtatCompression::default();
    for zone in cible.zones_swap()? {
        let type_zone = type_zone(&zone.chemin);
        etat.zram_presents |= type_zone == TypeZone::Zram;
        etat.zones.push(ZoneSwap {
            chemin: zone.chemin,
            type_zone,
            taille_ko: zone.taille_ko,
            priorite: zone.priorite,
        });
    }
    etat.generator_installe = cible.fichier_existe(Path::new(CHEMIN_GENERATEUR));
    etat.generator_confige = cible.fichier_existe(Path::new(CHEMIN_GENERATEUR_CONF));
    if let Ok(texte) = cible.lit_fichier(Path::new(CHEMIN_ZSWAP_ACTIVE)) {
        etat.zswap_actif = Some(matches!(texte.trim(), "Y" | "1"));
    }
    Ok(etat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_zone_par_chemin() {
        assert_eq!(type_zone("/dev/zram0"), TypeZone::Zram);
        assert_eq!(type_zone("/swapfile"), TypeZone::Disque);
        assert_eq!(type_zone("/dev/sda2"), TypeZone::Disque);
    }
}
