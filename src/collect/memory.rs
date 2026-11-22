use crate::collect::Collecteur;
use crate::error::{Error, Result};
use crate::file;

const TAMPON_MEMINFO: usize = 8192;
const TAMPON_LOADAVG: usize = 256;

pub fn collecte(e: &mut Collecteur) -> Result<()> {
    let mut tampon = [0u8; TAMPON_MEMINFO];
    let texte = file::read_proc(&e.chemins.meminfo, &mut tampon)?;
    let total = meminfo_valeur(texte, "MemTotal:").ok_or_else(|| Error::new("MemTotal absent"))?;
    let dispo =
        meminfo_valeur(texte, "MemAvailable:").ok_or_else(|| Error::new("MemAvailable absent"))?;
    let swap_t =
        meminfo_valeur(texte, "SwapTotal:").ok_or_else(|| Error::new("SwapTotal absent"))?;
    let swap_f = meminfo_valeur(texte, "SwapFree:").ok_or_else(|| Error::new("SwapFree absent"))?;
    if total == 0 {
        return Err(Error::new("MemTotal nul"));
    }
    e.mesures.mem_usage = (total - dispo) as f64 / total as f64 * 100.0;
    e.mesures.mem_disponible = dispo as f64 / 1024.0;
    e.mesures.swap_usage = if swap_t == 0 {
        0.0
    } else {
        (swap_t - swap_f) as f64 / swap_t as f64 * 100.0
    };
    charge(e)
}

fn charge(e: &mut Collecteur) -> Result<()> {
    let mut tampon = [0u8; TAMPON_LOADAVG];
    let texte = file::read_proc(&e.chemins.loadavg, &mut tampon)?;
    let token = texte.split_whitespace().next().unwrap_or("");
    let load = token
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| Error::new("load1 illisible"))?;
    e.mesures.load1 = load;
    Ok(())
}

fn meminfo_valeur(texte: &str, cle: &str) -> Option<u64> {
    let pos = texte.find(cle)?;
    let bytes = texte.as_bytes();
    let mut p = pos + cle.len();
    while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
        p += 1;
    }
    if p >= bytes.len() || !bytes[p].is_ascii_digit() {
        return None;
    }
    Some(crate::collect::nombre_u64(bytes, &mut p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_valeur_limites() {
        let texte = "MemTotal:       16000 kB\nMemAvailable:  8000 kB\n";
        assert_eq!(meminfo_valeur(texte, "MemTotal:"), Some(16_000));
        assert_eq!(meminfo_valeur(texte, "MemAvailable:"), Some(8_000));
        assert_eq!(meminfo_valeur(texte, "SwapTotal:"), None);
        assert_eq!(meminfo_valeur(texte, "MemFree:"), None);
        let texte = "MemTotal:\t900 kB\n";
        assert_eq!(meminfo_valeur(texte, "MemTotal:"), Some(900));
        let texte = "MemTotal: abc\n";
        assert_eq!(meminfo_valeur(texte, "MemTotal:"), None);
        let texte = "MemTotal:";
        assert_eq!(meminfo_valeur(texte, "MemTotal:"), None);
        assert_eq!(meminfo_valeur("", "MemTotal:"), None);
    }

    #[test]
    fn load_token_limite() {
        assert_eq!(
            "0.42 0.13 0.05 1/320 12345"
                .split_whitespace()
                .next()
                .expect("token")
                .parse::<f64>(),
            Ok(0.42)
        );
        assert!("abc".parse::<f64>().is_err());
        assert!("".parse::<f64>().is_err());
    }
}
