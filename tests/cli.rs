use std::process::Command;

fn sysguard(args: &[&str]) -> (i32, String, String) {
    let sortie = Command::new(env!("CARGO_BIN_EXE_sysguard"))
        .args(args)
        .output()
        .expect("binaire lancé");
    (
        sortie.status.code().expect("code de sortie"),
        String::from_utf8_lossy(&sortie.stdout).to_string(),
        String::from_utf8_lossy(&sortie.stderr).to_string(),
    )
}

fn fixture(nom: &str) -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(nom)
        .to_string_lossy()
        .to_string()
}

#[test]
fn version_et_aide() {
    let (code, out, _) = sysguard(&["version"]);
    assert_eq!(code, 0);
    assert_eq!(out, "sysguard 1.4.0\n");
    for aide in ["help", "-h", "--help"] {
        let (code, out, _) = sysguard(&[aide]);
        assert_eq!(code, 0, "{aide}");
        assert!(out.contains("usage : sysguard"), "{aide}");
        assert!(out.contains("dry-run"), "{aide}");
    }
}

#[test]
fn codes_erreur() {
    let (code, _, err) = sysguard(&["pigeon"]);
    assert_eq!(code, 2);
    assert!(err.contains("commande inconnue : pigeon"), "{err}");
    let (code, _, _) = sysguard(&["check", "version"]);
    assert_eq!(code, 2, "deux commandes");
    let (code, _, _) = sysguard(&["--config"]);
    assert_eq!(code, 2, "option nue");
}

#[test]
fn check_config() {
    let (code, out, _) = sysguard(&["check", "--config", &fixture("valide.yaml")]);
    assert_eq!(code, 0, "valide");
    assert!(
        out.contains("configuration valide : 3 regles, intervalle 15s"),
        "{out}"
    );
    let (code, out, _) = sysguard(&[&format!("--config={}", fixture("valide.yaml")), "check"]);
    assert_eq!(code, 0, "forme =");
    assert!(out.contains("configuration valide"), "{out}");
    let (code, _, err) = sysguard(&["check", "--config", &fixture("absente.yaml")]);
    assert_eq!(code, 1, "absente");
    assert!(err.contains("configuration invalide"), "{err}");
    let (code, _, _) = sysguard(&["check", "--config", "/etc/sysguard/sysguard.yaml"]);
    assert_eq!(code, 1, "défaut absente sur poste de dev");
}

#[test]
fn run_once_declenche() {
    let (code, out, _) = sysguard(&["--once", "--config", &fixture("daemon-once.yaml"), "run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("daemon démarré"), "{out}");
    assert!(out.contains("declenche une fois"), "{out}");
    assert!(out.contains("daemon arrêté"), "{out}");
}

#[test]
fn dry_run_once_ignore_exec() {
    let (code, out, _) = sysguard(&[
        "--once",
        "--config",
        &fixture("daemon-once.yaml"),
        "dry-run",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("dry-run : exec ignore"), "{out}");
    assert!(
        out.contains("declenche une fois"),
        "les actions log passent en dry-run : {out}"
    );
}

#[test]
fn run_config_absente_a_un() {
    let (code, out, _) = sysguard(&["run", "--config", &fixture("absente.yaml")]);
    assert_eq!(code, 1);
    assert!(out.contains("fichier illisible"), "{out}");
}

#[test]
fn optimise_sans_section_a_un() {
    let (code, out, _) = sysguard(&["optimise", "--once", "--config", &fixture("valide.yaml")]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("ni section optimisation ni section cpu"),
        "{out}"
    );
}

#[test]
fn optimise_once_degrade_propose() {
    // poste de dev sans privileges : les ecritures /proc/sys echouent,
    // l'optimiseur journalise des recommandations et sort a zero
    let (code, out, _) = sysguard(&["optimise", "--once", "--config", &fixture("optimise.yaml")]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("optimiseur démarré"), "{out}");
    assert!(out.contains("optimisation : vm.swappiness"), "{out}");
    assert!(out.contains("optimiseur arrêté"), "{out}");
}
