use std::ffi::CString;
use std::io;
use std::path::Path;

use crate::collect::StatfsInfo;

/// Appel direct de statfs(2) : std ne l'expose pas.
#[allow(unsafe_code)]
pub fn statfs(montage: &str) -> io::Result<StatfsInfo> {
    let chemin = CString::new(montage).map_err(|_| io::Error::other("chemin invalide"))?;
    let mut infos: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY : infos est un struct statfs complet et zéro, chemin vivant pendant l'appel.
    let rc = unsafe { libc::statfs(chemin.as_ptr(), &mut infos) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(StatfsInfo {
        bsize: u64::try_from(infos.f_bsize).unwrap_or(0),
        blocks: infos.f_blocks,
        bavail: infos.f_bavail,
        files: infos.f_files,
        ffree: infos.f_ffree,
    })
}

/// sysconf(_SC_CLK_TCK) : constantes valides, aucune mémoire pointeur.
#[allow(unsafe_code)]
pub fn ticks_par_seconde() -> i64 {
    // SAFETY : sysconf(_SC_CLK_TCK) ne prend que des valeurs constantes valides.
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) as i64 }
}

/// setpriority(PRIO_PROCESS, pid, nice) : arguments entiers validés.
#[allow(unsafe_code)]
pub fn renice(pid: i32, nice: i32) -> io::Result<()> {
    // SAFETY : setpriority(PRIO_PROCESS, pid) avec pid et nice valides.
    let rc = unsafe {
        libc::setpriority(
            libc::PRIO_PROCESS,
            u32::try_from(pid).unwrap_or(u32::MAX),
            nice,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// sched_setaffinity avec un cpu_set_t initialisé et des indices validés.
#[allow(unsafe_code)]
pub fn fixe_affinite(pid: i32, coeurs: &[usize]) -> io::Result<()> {
    // SAFETY : mask zéro puis CPU_SET sur des indices valides, taille exacte passée.
    let mut mask: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    unsafe { libc::CPU_ZERO(&mut mask) };
    for &coeur in coeurs {
        unsafe { libc::CPU_SET(coeur, &mut mask) };
    }
    let rc = unsafe { libc::sched_setaffinity(pid, std::mem::size_of::<libc::cpu_set_t>(), &mask) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Commande externe directe, sans shell, échec propagé.
pub fn commande(prog: &str, args: &[&str]) -> io::Result<()> {
    let statut = std::process::Command::new(prog)
        .args(args)
        .status()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::other(format!("{prog} absent"))
            } else {
                e
            }
        })?;
    if statut.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{prog} : code {}",
        statut.code().unwrap_or(-1)
    )))
}

/// swapon(2) avec priorité encodée dans swapflags (SWAP_FLAG_PREFER).
#[allow(unsafe_code)]
pub fn swapon(chemin: &Path, priorite: i32) -> io::Result<()> {
    use std::ffi::CString;
    let chemin_c = CString::new(
        chemin
            .to_str()
            .ok_or_else(|| io::Error::other("chemin invalide"))?,
    )
    .map_err(|_| io::Error::other("chemin invalide"))?;
    // Constantes de linux/swap.h absentes de la crate libc.
    const SWAP_FLAG_PREFER: i32 = 0x8000;
    const SWAP_FLAG_PRIO_MASK: i32 = 0x7fff;
    let drapeaux = SWAP_FLAG_PREFER | (priorite & SWAP_FLAG_PRIO_MASK);
    // SAFETY : chemin_c est un CString vivant pendant l'appel.
    let rc = unsafe { libc::swapon(chemin_c.as_ptr(), drapeaux) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub struct SourceReelle;

impl crate::collect::SourceStatfs for SourceReelle {
    fn statfs(&self, montage: &str) -> io::Result<StatfsInfo> {
        statfs(montage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statfs_reelle_racine() {
        let infos = statfs("/").expect("statfs /");
        assert!(infos.bsize > 0);
        assert!(infos.blocks > 0);
    }

    #[test]
    fn statfs_absente_erreur() {
        assert!(statfs("/inexistant-sysguard").is_err());
    }

    #[test]
    fn chemin_avec_octet_nul_refuse() {
        assert!(statfs("/a\0b").is_err());
    }
}
