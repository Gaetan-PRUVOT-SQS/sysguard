use crate::collect::{Collecteur, Disque, Mesures};
use crate::config::{Config, Metrique};
use crate::error::Result;

pub fn collecte(e: &mut Collecteur, cfg: &Config) -> Result<()> {
    e.mesures.disques.clear();
    for regle in &cfg.regles {
        let Some(metrique) = regle.condition.metrique else {
            continue;
        };
        if !metrique_disque(metrique) {
            continue;
        }
        let Some(montage) = &regle.condition.montage else {
            continue;
        };
        if connu(&e.mesures, montage) {
            continue;
        }
        if let Some(disque) = mesure(e.statfs, montage) {
            e.mesures.disques.push(disque);
        }
    }
    Ok(())
}

fn metrique_disque(m: Metrique) -> bool {
    matches!(
        m,
        Metrique::DiskUsage | Metrique::DiskFree | Metrique::DiskInodes
    )
}

fn connu(m: &Mesures, montage: &str) -> bool {
    m.disques.iter().any(|d| d.montage == montage)
}

fn mesure(source: &dyn crate::collect::SourceStatfs, montage: &str) -> Option<Disque> {
    let infos = source.statfs(montage).ok()?;
    let total = infos.blocks as f64 * infos.bsize as f64;
    let bloques = infos.bavail as f64 * infos.bsize as f64;
    let usage = if total > 0.0 {
        (total - bloques) / total * 100.0
    } else {
        0.0
    };
    let inodes = if infos.files > 0 {
        infos.files.wrapping_sub(infos.ffree) as f64 / infos.files as f64 * 100.0
    } else {
        0.0
    };
    Some(Disque {
        montage: montage.to_string(),
        usage,
        libre: bloques,
        inodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metriques_disques() {
        assert!(metrique_disque(Metrique::DiskUsage));
        assert!(metrique_disque(Metrique::DiskFree));
        assert!(metrique_disque(Metrique::DiskInodes));
        assert!(!metrique_disque(Metrique::CpuUsage));
        assert!(!metrique_disque(Metrique::Load1));
    }

    #[test]
    fn calculs_disque_zero() {
        struct SourceZero;
        impl crate::collect::SourceStatfs for SourceZero {
            fn statfs(&self, _: &str) -> std::io::Result<crate::collect::StatfsInfo> {
                Ok(crate::collect::StatfsInfo {
                    bsize: 0,
                    blocks: 0,
                    bavail: 0,
                    files: 0,
                    ffree: 0,
                })
            }
        }
        let d = mesure(&SourceZero, "/").expect("disque");
        assert_eq!(d.usage, 0.0);
        assert_eq!(d.libre, 0.0);
        assert_eq!(d.inodes, 0.0);
    }
}
