use std::ffi::OsString;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};

const TAILLE_SUN_PATH: usize = 108;

pub fn envoi(message: &str) -> io::Result<()> {
    let chemin = std::env::var_os("NOTIFY_SOCKET");
    envoi_chemin(chemin, message)
}

pub fn envoi_chemin(chemin: Option<OsString>, message: &str) -> io::Result<()> {
    let Some(chemin) = chemin else {
        return Ok(());
    };
    let chemin = chemin.to_string_lossy().to_string();
    if chemin.is_empty() || chemin.len() >= TAILLE_SUN_PATH {
        return Ok(());
    }
    let adresse = if let Some(nom) = chemin.strip_prefix('@') {
        SocketAddr::from_abstract_name(nom.as_bytes())?
    } else {
        SocketAddr::from_pathname(chemin.as_str())?
    };
    let socket = UnixDatagram::unbound()?;
    socket.send_to_addr(message.as_bytes(), &adresse)?;
    Ok(())
}

pub fn delai_watchdog() -> i64 {
    delai_watchdog_depuis(std::env::var("WATCHDOG_USEC").ok().as_deref())
}

pub fn delai_watchdog_depuis(valeur: Option<&str>) -> i64 {
    let Some(valeur) = valeur else {
        return 0;
    };
    if valeur.is_empty() {
        return 0;
    }
    match valeur.trim().parse::<i64>() {
        Ok(usec) if usec > 0 => usec / 3,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemin_absent_ou_vide_passe_sans_envoi() {
        assert!(envoi_chemin(None, "READY=1").is_ok());
        assert!(envoi_chemin(Some(OsString::from("")), "READY=1").is_ok());
    }

    #[test]
    fn chemin_trop_long_ignore() {
        let long = "a".repeat(TAILLE_SUN_PATH);
        assert!(envoi_chemin(Some(OsString::from(long)), "READY=1").is_ok());
    }

    #[test]
    fn envoi_vers_socket_reelle() {
        use std::os::unix::net::UnixDatagram;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("build");
        std::fs::create_dir_all(&dir).expect("build dir");
        let chemin = dir.join("notify-test.sock");
        let _ = std::fs::remove_file(&chemin);
        let recepteur = UnixDatagram::bind(&chemin).expect("bind");
        envoi_chemin(
            Some(OsString::from(chemin.to_string_lossy().to_string())),
            "READY=1",
        )
        .expect("envoi");
        let mut tampon = [0u8; 64];
        let (n, _) = recepteur.recv_from(&mut tampon).expect("réception");
        assert_eq!(String::from_utf8_lossy(&tampon[..n]), "READY=1");
        let _ = std::fs::remove_file(&chemin);
    }

    #[test]
    fn delai_watchdog_bornes() {
        assert_eq!(delai_watchdog_depuis(None), 0);
        assert_eq!(delai_watchdog_depuis(Some("")), 0);
        assert_eq!(delai_watchdog_depuis(Some("abc")), 0);
        assert_eq!(delai_watchdog_depuis(Some("-5")), 0);
        assert_eq!(delai_watchdog_depuis(Some("0")), 0);
        assert_eq!(delai_watchdog_depuis(Some("900000")), 300000);
        assert_eq!(delai_watchdog_depuis(Some("60000000 \n")), 20000000);
    }
}
