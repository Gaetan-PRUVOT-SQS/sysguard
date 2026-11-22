use std::path::Path;

use sysguard::compression::{CHEMIN_ZSWAP_ACTIVE, detecte};
use sysguard::optimisation::{CibleReelle, CibleReglage};

#[test]
fn lectures_reelles_proc() {
    let cible = CibleReelle;
    let pression = cible.lit_pression().expect("PSI mémoire lisible");
    assert!(pression.some >= 0.0);
    let pression_cpu = cible.lit_pression_cpu().expect("PSI cpu lisible");
    assert!(pression_cpu.some >= 0.0);
    let load = cible.lit_load1().expect("loadavg lisible");
    assert!(load >= 0.0);
    assert!(cible.ram_octets() > 0, "MemTotal lu");
    assert!(cible.processus().expect("processus").len() > 10);
    assert!(cible.zones_swap().is_ok());
    assert!(
        cible.fichier_existe(Path::new("/proc/self/stat")),
        "fichier_existe réel"
    );
}

#[test]
fn liste_cpus_reelle_ou_vide() {
    let cible = CibleReelle;
    let cpus = cible.liste_cpus();
    // VM/containers sans cpufreq : la liste peut être vide, jamais fausse
    for cpu in &cpus {
        assert!(
            cpu.to_string_lossy()
                .starts_with("/sys/devices/system/cpu/cpu")
        );
    }
}

#[test]
fn renice_et_affinite_sur_soi_meme() {
    let pid = std::process::id() as i32;
    // ramener son propre nice à 0 est toujours permis sans privilège
    CibleReelle.renice(pid, 0).expect("renice sur soi-même");
    let coeurs: Vec<usize> = (0..4).collect();
    CibleReelle
        .fixe_affinite(pid, &coeurs)
        .expect("affinité sur soi-même");
}

#[test]
fn renice_pid_inexistant_erreur() {
    assert!(CibleReelle.renice(i32::MAX, 5).is_err());
}

#[test]
fn swapon_chemin_invalide_erreur() {
    assert!(
        CibleReelle
            .swapon(Path::new("/inexistant-sysguard-swap"), 100)
            .is_err()
    );
}

#[test]
fn commande_absente_et_ok() {
    assert!(
        CibleReelle
            .lance_commande("sysguard-inexistant", &[])
            .is_err()
    );
    assert!(CibleReelle.lance_commande("true", &[]).is_ok());
}

#[test]
fn detection_reelle() {
    let cible = CibleReelle;
    let etat = detecte(&cible as &dyn CibleReglage).expect("état");
    // cohérence : une zone zram implique zram_presents
    for zone in &etat.zones {
        if zone.chemin.starts_with("/dev/zram") {
            assert!(etat.zram_presents);
        }
    }
    let _ = cible.lit_fichier(Path::new(CHEMIN_ZSWAP_ACTIVE));
}
