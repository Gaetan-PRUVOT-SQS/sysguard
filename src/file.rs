use std::fs::File;
use std::io;
use std::io::Read;
use std::path::Path;

use crate::error::{Error, Result};

pub const TAILLE_MAX: u64 = 16_777_216;

pub fn read_file(path: &Path) -> Result<String> {
    let mut f = File::open(path).map_err(|e| erreur_io(path, &e))?;
    let taille = f.metadata().map_err(|e| erreur_io(path, &e))?.len();
    verifier_taille(taille)?;
    let mut tampon = vec![0u8; usize::try_from(taille).expect("taille bornée")];
    lire_exact(&mut f, &mut tampon).map_err(|e| erreur_io(path, &e))?;
    String::from_utf8(tampon).map_err(|_| Error::new("fichier non utf-8"))
}

pub fn read_proc<'a>(path: &Path, tampon: &'a mut [u8]) -> Result<&'a str> {
    if tampon.len() < 2 {
        return Err(Error::new("tampon trop petit"));
    }
    let mut f = File::open(path).map_err(|e| erreur_io(path, &e))?;
    let n = lire_jusqua(&mut f, tampon).map_err(|e| erreur_io(path, &e))?;
    std::str::from_utf8(&tampon[..n]).map_err(|_| Error::new("contenu non utf-8"))
}

fn verifier_taille(taille: u64) -> Result<()> {
    if taille == 0 || taille > TAILLE_MAX {
        return Err(Error::new("taille de fichier refusee"));
    }
    Ok(())
}

fn erreur_io(path: &Path, e: &io::Error) -> Error {
    Error::new(format!("{} : {}", path.display(), e))
}

fn lire_exact(f: &mut File, tampon: &mut [u8]) -> io::Result<()> {
    let mut total = 0;
    while total < tampon.len() {
        match f.read(&mut tampon[total..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn lire_jusqua(f: &mut File, tampon: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < tampon.len() {
        match f.read(&mut tampon[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(nom: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(nom)
    }

    #[test]
    fn taille_refusee_aux_bornes() {
        assert!(verifier_taille(0).is_err());
        assert!(verifier_taille(1).is_ok());
        assert!(verifier_taille(TAILLE_MAX).is_ok());
        assert!(verifier_taille(TAILLE_MAX + 1).is_err());
    }

    #[test]
    fn fichier_lu() {
        let contenu = read_file(&fixture("petit.txt")).expect("lecture");
        assert_eq!(contenu, "contenu de test\n");
    }

    #[test]
    fn fichier_vide_refuse() {
        assert!(read_file(&fixture("vide")).is_err());
    }

    #[test]
    fn fichier_absent_refuse() {
        assert!(read_file(&fixture("absente.yaml")).is_err());
    }

    #[test]
    fn fichier_non_utf8_refuse() {
        assert!(read_file(&fixture("binaire")).is_err());
    }

    #[test]
    fn proc_lu_dans_tampon() {
        let mut tampon = [0u8; 256];
        let contenu = read_proc(Path::new("/proc/self/cmdline"), &mut tampon).expect("lecture");
        assert!(!contenu.is_empty());
    }

    #[test]
    fn proc_tronque_a_la_taille_du_tampon() {
        let mut tampon = [0u8; 64];
        let lu = read_proc(Path::new("/proc/meminfo"), &mut tampon).expect("lecture");
        assert_eq!(lu.len(), 64);
    }

    #[test]
    fn tampon_trop_petit_refuse() {
        let mut tampon = [0u8; 1];
        assert!(read_proc(Path::new("/proc/self/cmdline"), &mut tampon).is_err());
    }
}
