use crate::error::{Error, Result};

const UNITE_MS: i64 = 1;
const UNITE_S: i64 = 1_000;
const UNITE_M: i64 = 60_000;
const UNITE_H: i64 = 3_600_000;

pub fn parse_duration(text: &str) -> Result<i64> {
    if text == "0" {
        return Ok(0);
    }
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return Err(Error::new("durée invalide"));
    }
    let mut i = 0;
    let mut total: i64 = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            return Err(Error::new("durée invalide"));
        }
        let mut nombre: i64 = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            let chiffre = i64::from(bytes[i] - b'0');
            nombre = nombre
                .checked_mul(10)
                .and_then(|n| n.checked_add(chiffre))
                .ok_or_else(|| Error::new("durée invalide"))?;
            i += 1;
        }
        let (mult, avance) = if bytes[i..].starts_with(b"ms") {
            (UNITE_MS, 2)
        } else if i < bytes.len() && bytes[i] == b's' {
            (UNITE_S, 1)
        } else if i < bytes.len() && bytes[i] == b'm' {
            (UNITE_M, 1)
        } else if i < bytes.len() && bytes[i] == b'h' {
            (UNITE_H, 1)
        } else {
            return Err(Error::new("durée invalide"));
        };
        i += avance;
        let partie = nombre
            .checked_mul(mult)
            .ok_or_else(|| Error::new("durée invalide"))?;
        total = total
            .checked_add(partie)
            .ok_or_else(|| Error::new("durée invalide"))?;
    }
    Ok(total)
}

pub fn format_duration(ms: i64) -> String {
    if ms % 1000 != 0 {
        return format!("{ms}ms");
    }
    let s = ms / 1000;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let s = s % 60;
    if h > 0 {
        format!("{h}h{m}m{s}s")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refuse(texte: &str) {
        assert!(parse_duration(texte).is_err(), "refus attendu : {texte}");
    }

    #[test]
    fn accepted_forms() {
        assert_eq!(parse_duration("15s").expect("valide"), 15_000);
        assert_eq!(parse_duration("2m").expect("valide"), 120_000);
        assert_eq!(parse_duration("1h").expect("valide"), 3_600_000);
        assert_eq!(parse_duration("500ms").expect("valide"), 500);
        assert_eq!(parse_duration("1m30s").expect("valide"), 90_000);
        assert_eq!(parse_duration("0").expect("valide"), 0);
        assert_eq!(parse_duration("1h30m20s").expect("valide"), 5_420_000);
    }

    #[test]
    fn refused_forms() {
        refuse("15x");
        refuse("1.5s");
        refuse("-5s");
        refuse("");
        refuse("s");
        refuse("15");
        refuse("99999999999999999999s");
        refuse("10000000000000000s");
        refuse("sm");
    }

    #[test]
    fn overflow_boundary() {
        assert_eq!(
            parse_duration("9223372036854775807ms").expect("borne i64"),
            i64::MAX
        );
        refuse("9223372036854775808ms");
    }

    #[test]
    fn metamorphique_aller_retour() {
        // parse(format(v)) == v pour toute valeur représentable non nulle
        // et multiple de 1000 (formes canoniques h/m/s) ou en ms pures.
        let valeurs = [
            0,
            1,
            500,
            999,
            1_000,
            1_500,
            59_999,
            60_000,
            3_599_999,
            3_600_000,
            86_400_000,
            7_776_000_000,
        ];
        for v in valeurs {
            let texte = format_duration(v);
            let retour = parse_duration(&texte).expect(&texte);
            assert_eq!(retour, v, "aller-retour de {v} via {texte}");
        }
    }

    #[test]
    fn supposition_erreur_formes_typiques() {
        // Technique supposition d'erreur : fautes de frappe réalistes
        refuse("1 s");
        refuse("1s ");
        refuse(" 1s");
        refuse("1S");
        refuse("1m0");
        refuse("s5");
        refuse("1m s");
        refuse("++5s");
    }

    #[test]
    fn formatted() {
        assert_eq!(format_duration(15_000), "15s");
        assert_eq!(format_duration(120_000), "2m0s");
        assert_eq!(format_duration(90_000), "1m30s");
        assert_eq!(format_duration(3_600_000), "1h0m0s");
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(0), "0s");
    }
}
