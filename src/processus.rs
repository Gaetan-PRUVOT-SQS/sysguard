use std::path::Path;

use crate::error::{Error, Result};
use crate::file;

#[derive(Debug, PartialEq, Clone)]
pub struct ProcessusCpu {
    pub pid: i32,
    pub comm: String,
    pub ticks: u64,
    pub nice: i64,
}

/// Parse /proc/[pid]/stat : comm peut contenir espaces et parenthèses,
/// on coupe après la dernière parenthèse fermante.
pub fn parse_stat(texte: &str) -> Result<ProcessusCpu> {
    let ouvrante = texte
        .find('(')
        .ok_or_else(|| Error::new("stat illisible"))?;
    let fermante = texte
        .rfind(')')
        .ok_or_else(|| Error::new("stat illisible"))?;
    if fermante < ouvrante {
        return Err(Error::new("stat illisible"));
    }
    let pid: i32 = texte[..ouvrante]
        .trim()
        .parse()
        .map_err(|_| Error::new("stat illisible"))?;
    let comm = texte[ouvrante + 1..fermante].to_string();
    let champs: Vec<&str> = texte[fermante + 2..].split_whitespace().collect();
    let item = |i: usize| -> Result<&str> {
        champs
            .get(i)
            .copied()
            .ok_or_else(|| Error::new("stat illisible"))
    };
    // après la comm : state=0, ppid=1, ..., utime=11, stime=12, ..., nice=17
    let utime: u64 = item(11)?
        .parse()
        .map_err(|_| Error::new("stat illisible"))?;
    let stime: u64 = item(12)?
        .parse()
        .map_err(|_| Error::new("stat illisible"))?;
    let nice: i64 = item(16)?
        .parse()
        .map_err(|_| Error::new("stat illisible"))?;
    Ok(ProcessusCpu {
        pid,
        comm,
        ticks: utime + stime,
        nice,
    })
}

pub fn liste_processus(rep: &Path, tampon: &mut [u8]) -> Result<Vec<ProcessusCpu>> {
    let mut liste = Vec::new();
    let entrees = std::fs::read_dir(rep).map_err(|e| Error::new(format!("/proc : {e}")))?;
    for entree in entrees {
        let Ok(entree) = entree else { continue };
        let nom = entree.file_name();
        let Some(nom) = nom.to_str() else { continue };
        if !nom.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let chemin = rep.join(nom).join("stat");
        if let Ok(texte) = file::read_proc(&chemin, tampon)
            && let Ok(p) = parse_stat(texte)
        {
            liste.push(p);
        }
    }
    Ok(liste)
}

#[derive(Debug, PartialEq, Clone)]
pub struct Consommateur {
    pub pid: i32,
    pub comm: String,
    pub cpu_ms: u64,
    pub nice: i64,
}

/// Delta de ticks entre deux passes, converti en ms avec ticks/s.
/// Les pid absents de la seconde passe sont ignorés (processus finis).
pub fn top_consommateurs(
    avant: &[ProcessusCpu],
    apres: &[ProcessusCpu],
    ticks_par_seconde: i64,
    nombre: usize,
) -> Vec<Consommateur> {
    let mut deltas: Vec<Consommateur> = apres
        .iter()
        .filter_map(|p2| {
            let p1 = avant.iter().find(|p1| p1.pid == p2.pid)?;
            let delta = p2.ticks.saturating_sub(p1.ticks);
            let ms = delta
                .checked_mul(1000)
                .and_then(|d| d.checked_div(ticks_par_seconde.max(1) as u64))
                .unwrap_or(0);
            Some(Consommateur {
                pid: p2.pid,
                comm: p2.comm.clone(),
                cpu_ms: ms,
                nice: p2.nice,
            })
        })
        .collect();
    deltas.sort_by(|a, b| b.cpu_ms.cmp(&a.cpu_ms).then(a.pid.cmp(&b.pid)));
    deltas.truncate(nombre);
    deltas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_simple() {
        let p = parse_stat("123 (monapp) S 1 123 123 0 -1 4194560 0 0 0 0 100 50 0 0 20 0 1 0 12345 12345 120 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 1 0 0 0 0 0\n")
            .expect("valide");
        assert_eq!(p.pid, 123);
        assert_eq!(p.comm, "monapp");
        assert_eq!(p.ticks, 150);
        assert_eq!(p.nice, 0, "nice (19e champ) = 0, priority = 20");
    }

    #[test]
    fn stat_comm_avec_parentheses_et_espaces() {
        let p = parse_stat("42 (mon (super) app v2) R 1 42 42 0 -1 0 0 0 0 0 7 3 0 0 5 0 2 0 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n")
            .expect("valide");
        assert_eq!(p.pid, 42);
        assert_eq!(p.comm, "mon (super) app v2");
        assert_eq!(p.ticks, 10);
        assert_eq!(p.nice, 0);
    }

    #[test]
    fn stat_illisible() {
        assert!(parse_stat("").is_err());
        assert!(parse_stat("123 sans-comm ici\n").is_err());
        assert!(parse_stat("(x) y\n").is_err());
        assert!(parse_stat("abc (x) y 1 2 3\n").is_err());
        assert!(parse_stat("123 (x) y 1 2 3\n").is_err(), "champs manquants");
    }

    #[test]
    fn deltas_et_tri() {
        let avant = vec![
            ProcessusCpu {
                pid: 1,
                comm: "gros".into(),
                ticks: 0,
                nice: 0,
            },
            ProcessusCpu {
                pid: 2,
                comm: "petit".into(),
                ticks: 0,
                nice: 0,
            },
            ProcessusCpu {
                pid: 3,
                comm: "fini".into(),
                ticks: 999,
                nice: 0,
            },
        ];
        let apres = vec![
            ProcessusCpu {
                pid: 2,
                comm: "petit".into(),
                ticks: 10,
                nice: 0,
            },
            ProcessusCpu {
                pid: 1,
                comm: "gros".into(),
                ticks: 300,
                nice: 0,
            },
            ProcessusCpu {
                pid: 4,
                comm: "nouveau".into(),
                ticks: 500,
                nice: 0,
            },
        ];
        let top = top_consommateurs(&avant, &apres, 100, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].comm, "gros");
        assert_eq!(top[0].cpu_ms, 3000, "300 ticks à 100/s");
        assert_eq!(top[1].comm, "petit");
        assert_eq!(top[1].cpu_ms, 100);
    }

    #[test]
    fn ticks_borne() {
        let avant = vec![ProcessusCpu {
            pid: 1,
            comm: "a".into(),
            ticks: 999,
            nice: 0,
        }];
        let apres = vec![ProcessusCpu {
            pid: 1,
            comm: "a".into(),
            ticks: 5,
            nice: 0,
        }];
        let top = top_consommateurs(&avant, &apres, 100, 1);
        assert_eq!(top[0].cpu_ms, 0, "recul de compteur borné à 0");
    }
}
