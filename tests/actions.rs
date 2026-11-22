use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use sysguard::action::{ActionCours, ContexteAction, dry_run, executer, log_action, niveau_action};
use sysguard::config::{
    Action, Condition, Config, Metrique, Operateur, Regle, Severite, TypeAction,
};
use sysguard::log::{Level, Logger};

fn regle(nom: &str, severite: Severite) -> Regle {
    Regle {
        nom: nom.to_string(),
        severite,
        condition: Condition {
            metrique: Some(Metrique::CpuUsage),
            montage: None,
            operateur: Some(Operateur::Sup),
            valeur: 50.0,
        },
        pour: 0,
        cooldown: 0,
        actions: Vec::new(),
    }
}

fn ctx(dry_run: bool) -> ContexteAction {
    ContexteAction {
        dry_run,
        hostname: "hote-local".to_string(),
    }
}

fn act_log(message: Option<&str>, niveau: Option<Level>) -> Action {
    Action {
        kind: TypeAction::Log,
        timeout: None,
        url: None,
        command: None,
        message: message.map(str::to_string),
        niveau_log: niveau,
    }
}

#[test]
fn niveaux_log_par_severite() {
    let r = regle("essai", Severite::Critical);
    let a = act_log(None, None);
    assert_eq!(niveau_action(&a, &r), Level::Error);
    let r = regle("essai", Severite::Info);
    assert_eq!(niveau_action(&a, &r), Level::Info);
    let r = regle("essai", Severite::Warning);
    assert_eq!(niveau_action(&a, &r), Level::Warn);
    let a = act_log(None, Some(Level::Debug));
    assert_eq!(niveau_action(&a, &r), Level::Debug);
}

#[test]
fn action_log_message_defaut_et_template() {
    let r = regle("essai", Severite::Critical);
    let a = act_log(None, None);
    let contexte = ctx(false);
    let mut journal = Logger::new("debug", Vec::new()).expect("journal");
    let ac = ActionCours {
        act: &a,
        regle: &r,
        valeur: 75.5,
    };
    log_action(&ac, &mut journal, &contexte).expect("log");
    let out = String::from_utf8(journal.contenu()).expect("utf-8");
    assert!(out.contains("\"level\":\"ERROR\""), "{out}");
    assert!(out.contains("regle déclenchée"), "{out}");

    let a = act_log(Some("info {{.Value}} sur {{.Hostname}}"), None);
    let r = regle("essai", Severite::Info);
    let mut journal = Logger::new("info", Vec::new()).expect("journal");
    let ac = ActionCours {
        act: &a,
        regle: &r,
        valeur: 75.5,
    };
    log_action(&ac, &mut journal, &contexte).expect("log");
    let out = String::from_utf8(journal.contenu()).expect("utf-8");
    assert!(out.contains("info 75.5 sur hote-local"), "{out}");
    assert!(out.contains("\"level\":\"INFO\""), "{out}");
}

#[test]
fn dry_run_ignore_webhook_et_exec() {
    let r = regle("essai", Severite::Warning);
    let mut journal = Logger::new("info", Vec::new()).expect("journal");

    let mut a_webhook = act_log(None, None);
    a_webhook.kind = TypeAction::Webhook;
    a_webhook.url = Some("http://127.0.0.1:1/x".to_string());
    let ac = ActionCours {
        act: &a_webhook,
        regle: &r,
        valeur: 75.5,
    };
    dry_run(&ac, &mut journal).expect("dry-run webhook");

    let mut a_exec = act_log(None, None);
    a_exec.kind = TypeAction::Exec;
    a_exec.command = Some("/bin/true".to_string());
    let ac = ActionCours {
        act: &a_exec,
        regle: &r,
        valeur: 75.5,
    };
    dry_run(&ac, &mut journal).expect("dry-run exec");

    let out = String::from_utf8(journal.contenu()).expect("utf-8");
    assert!(out.contains("webhook ignore"), "{out}");
    assert!(out.contains("exec ignore"), "{out}");
}

#[test]
fn dispatch_dry_run_passe_par_dry_run() {
    let r = regle("essai", Severite::Warning);
    let mut a = act_log(None, None);
    a.kind = TypeAction::Webhook;
    a.url = Some("http://127.0.0.1:1/x".to_string());
    let cfg = Config::default();
    let contexte = ctx(true);
    let mut journal = Logger::new("info", Vec::new()).expect("journal");
    let ac = ActionCours {
        act: &a,
        regle: &r,
        valeur: 75.5,
    };
    executer(&ac, &mut journal, &cfg, &contexte).expect("dry-run dispatch");
    let out = String::from_utf8(journal.contenu()).expect("utf-8");
    assert!(out.contains("webhook ignore"), "{out}");
}

struct Serveur {
    port: u16,
    demandes: mpsc::Receiver<String>,
}

fn serveur(code: u16, texte: &str) -> Serveur {
    let ecouteur = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = ecouteur.local_addr().expect("adresse").port();
    let texte = texte.to_string();
    let (envoyeur, receveur) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut flux, _) = ecouteur.accept().expect("accept");
        let mut tampon = [0u8; 2048];
        let n = flux.read(&mut tampon).unwrap_or(0);
        let demande = tampon[..n].to_vec();
        let reponse = format!("HTTP/1.1 {code} {texte}\r\nContent-Length: 2\r\n\r\nok");
        let _ = flux.write_all(reponse.as_bytes());
        envoyeur
            .send(String::from_utf8_lossy(&demande).to_string())
            .ok();
    });
    Serveur {
        port,
        demandes: receveur,
    }
}

fn action_webhook(url: &str, message: Option<&str>, timeout: i64) -> (Action, Regle) {
    let mut a = act_log(message, None);
    a.kind = TypeAction::Webhook;
    a.url = Some(url.to_string());
    a.timeout = Some(timeout);
    (a, regle("web", Severite::Warning))
}

fn webhook_vers_port(port: u16, message: Option<&str>) -> (Action, Regle) {
    action_webhook(&format!("http://127.0.0.1:{port}/hook"), message, 2000)
}

#[test]
fn webhook_codes_et_requete() {
    let cfg = Config::default();
    let contexte = ctx(false);

    for (code, attendu) in [(200, true), (201, true), (204, true), (500, false)] {
        let texte = if code == 500 { "KO" } else { "OK" };
        let srv = serveur(code, texte);
        let (a, r) = webhook_vers_port(srv.port, Some("alerte \"prioritaire\" \\retour"));
        let mut journal = Logger::new("info", Vec::new()).expect("journal");
        let ac = ActionCours {
            act: &a,
            regle: &r,
            valeur: 80.0,
        };
        let resultat = executer(&ac, &mut journal, &cfg, &contexte);
        let demande = srv.demandes.recv().expect("demande reçue");
        assert!(
            demande.contains("Content-Type: application/json"),
            "{demande}"
        );
        assert!(demande.contains("POST /hook HTTP/1.1"), "{demande}");
        assert!(demande.contains("alerte \\\"prioritaire\\\""), "{demande}");
        assert!(demande.contains("\\\\retour"), "{demande}");
        assert_eq!(resultat.is_ok(), attendu, "code {code}");
    }
}

#[test]
fn webhook_refus_deterministes() {
    let cfg = Config::default();
    let contexte = ctx(false);
    for url in [
        "https://exemple.invalid/hook",
        "http://127.0.0.1:1/hook",
        "http://",
        "http://127.0.0.1:/hook",
        "http://hote-inexistant-xyz.invalid:80/hook",
        "http://hote:99999999/x",
    ] {
        let (a, r) = action_webhook(url, None, 2000);
        let mut journal = Logger::new("info", Vec::new()).expect("journal");
        let ac = ActionCours {
            act: &a,
            regle: &r,
            valeur: 80.0,
        };
        assert!(
            executer(&ac, &mut journal, &cfg, &contexte).is_err(),
            "{url} : refus attendu"
        );
    }
}

#[test]
fn webhook_timeout_par_action_prend_le_pas() {
    // délai global absurde (3e9 ms) : le timeout de l'action (2000 ms) doit primer
    let cfg = Config {
        webhook_timeout: 3_000_000_000,
        ..Config::default()
    };
    let srv = serveur(200, "OK");
    let (a, r) = webhook_vers_port(srv.port, None);
    let mut journal = Logger::new("info", Vec::new()).expect("journal");
    let contexte = ctx(false);
    let ac = ActionCours {
        act: &a,
        regle: &r,
        valeur: 80.0,
    };
    executer(&ac, &mut journal, &cfg, &contexte).expect("webhook ok");
    assert!(srv.demandes.recv().is_ok());
}

fn act_exec(command: &str, timeout: i64) -> (Action, Regle) {
    let mut a = act_log(None, None);
    a.kind = TypeAction::Exec;
    a.command = Some(command.to_string());
    a.timeout = if timeout < 0 { None } else { Some(timeout) };
    (a, regle("exec", Severite::Warning))
}

fn run_exec(command: &str, timeout: i64) -> (Result<(), sysguard::error::Error>, String) {
    let cfg = Config::default();
    let contexte = ctx(false);
    let (a, r) = act_exec(command, timeout);
    let mut journal = Logger::new("info", Vec::new()).expect("journal");
    let ac = ActionCours {
        act: &a,
        regle: &r,
        valeur: 80.0,
    };
    let resultat = executer(&ac, &mut journal, &cfg, &contexte);
    let out = String::from_utf8(journal.contenu()).expect("utf-8");
    (resultat, out)
}

#[test]
fn exec_sorties() {
    let (r, _) = run_exec("/bin/true", -1);
    assert!(r.is_ok(), "bin true réussit");
    let (r, _) = run_exec("/bin/false", -1);
    assert!(r.is_err(), "bin false échoue");
    let (r, out) = run_exec("bin/relatif", -1);
    assert!(r.is_err(), "chemin relatif refusé");
    assert!(out.contains("action exec : commande invalide"), "{out}");
    let (r, out) = run_exec("", -1);
    assert!(r.is_err(), "commande vide refusée");
    assert!(out.contains("commande invalide"), "{out}");
    let (r, _) = run_exec("/inexistant/binaire", -1);
    assert!(r.is_err(), "binaire absent échoue");
}

#[test]
fn exec_timeout_et_delai_global() {
    let debut = std::time::Instant::now();
    let (r, _) = run_exec("/bin/sleep 5", 300);
    assert!(r.is_err(), "timeout déclenche erreur");
    let ecoule = debut.elapsed();
    assert!(
        ecoule >= Duration::from_millis(250) && ecoule < Duration::from_secs(2),
        "durée {ecoule:?}"
    );
    let (r, _) = run_exec("/bin/true", -1);
    assert!(r.is_ok(), "timeout global hérité");
}

#[test]
fn exec_variables_sg_transmises() {
    let sortie =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sortie-exec.txt");
    let _ = std::fs::remove_file(&sortie);
    let (r, _) = run_exec("/bin/sh tests/fixtures/sg-ecrit.sh", 3000);
    assert!(r.is_ok(), "script env réussit");
    let texte = std::fs::read_to_string(&sortie).expect("sortie écrite");
    assert!(texte.contains("exec warning 80"), "{texte}");
    let _ = std::fs::remove_file(&sortie);
}

#[test]
fn exec_env_limite_aux_variables_sg() {
    // env_clear : le fils ne voit pas PATH, seulement les SG_*
    let sortie =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sortie-env.txt");
    let _ = std::fs::remove_file(&sortie);
    let (r, _) = run_exec("/bin/sh tests/fixtures/sg-env.sh", 3000);
    assert!(r.is_ok(), "script env réussit");
    let texte = std::fs::read_to_string(&sortie).expect("sortie écrite");
    assert_eq!(texte, "home= rule=exec\n");
    let _ = std::fs::remove_file(&sortie);
}
