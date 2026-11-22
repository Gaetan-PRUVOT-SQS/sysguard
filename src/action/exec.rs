use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::action::{ActionCours, ContexteAction, variables};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::log::{Level, Logger};

const DELAI_SONDAGE: u64 = 50;

pub fn exec<W: Write>(
    ac: &ActionCours,
    journal: &mut Logger<W>,
    cfg: &Config,
    ctx: &ContexteAction,
) -> Result<()> {
    let Some(command) = ac.act.command.as_deref() else {
        return Err(Error::new("action exec : commande absente"));
    };
    let v = variables(ac.regle, ac.valeur, &ctx.hostname);
    let cmd = crate::action::template_applique(command, &v);
    let argv = decoupe(&cmd);
    if argv.is_empty() || !argv[0].starts_with('/') {
        journal.write(Level::Error, "action exec : commande invalide");
        return Err(Error::new("action exec : commande invalide"));
    }
    let mut commande = Command::new(argv[0]);
    commande
        .args(&argv[1..])
        .env_clear()
        .process_group(0)
        .env("SG_RULE_NAME", &v.nom)
        .env("SG_SEVERITY", v.severite)
        .env("SG_VALUE", &v.valeur)
        .env("SG_THRESHOLD", &v.seuil)
        .env("SG_HOSTNAME", &v.hostname)
        .env("SG_TIMESTAMP", &v.horodatage);
    let mut enfant = commande
        .spawn()
        .map_err(|_| Error::new(format!("exec impossible : {}", argv[0])))?;
    let delai = delai_exec(ac, cfg);
    attendre(&mut enfant, delai)
}

fn delai_exec(ac: &ActionCours, cfg: &Config) -> i64 {
    match ac.act.timeout {
        Some(t) if t >= 0 => t,
        _ => cfg.exec_timeout,
    }
}

fn attendre(enfant: &mut std::process::Child, delai_ms: i64) -> Result<()> {
    let debut = Instant::now();
    loop {
        if debut.elapsed().as_millis() as i64 >= delai_ms {
            break;
        }
        thread::sleep(Duration::from_millis(DELAI_SONDAGE));
        match enfant.try_wait() {
            Ok(Some(statut)) if statut.code() == Some(0) => return Ok(()),
            Ok(Some(_)) => return Err(Error::new("action exec : code non nul")),
            Ok(None) => {}
            Err(e) => return Err(Error::new(format!("action exec : {e}"))),
        }
    }
    // Le timeout tue le groupe entier : la descendance lancée par la
    // commande meurt avec elle (l'enfant est leader de son groupe).
    let pid = enfant.id();
    if !crate::sys::tuer_groupe(pid) {
        let _ = enfant.kill();
    }
    enfant
        .wait()
        .map_err(|e| Error::new(format!("action exec : {e}")))?;
    Err(Error::new("action exec : delai depasse"))
}

pub(crate) fn decoupe(cmd: &str) -> Vec<&str> {
    cmd.split([' ', '\t'])
        .filter(|morcean| !morcean.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupe_par_espaces_et_tabs() {
        assert_eq!(
            decoupe("/bin/sh /x.sh a b"),
            vec!["/bin/sh", "/x.sh", "a", "b"]
        );
        assert_eq!(decoupe("  /bin/true  "), vec!["/bin/true"]);
        assert!(decoupe("").is_empty());
        assert!(decoupe("   \t ").is_empty());
        assert_eq!(decoupe("/a\tb c"), vec!["/a", "b", "c"]);
    }

    #[test]
    fn timeout_tue_la_descendance() {
        // Un script qui lance un arrière-plan (descendance) puis dort :
        // le timeout doit tuer les deux, pas seulement le leader.
        let dir = format!("{}/target/test-exec", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&dir).expect("repertoire de trace");
        let script = format!("{dir}/descendance.sh");
        let trace = format!("{dir}/descendance.pid");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nsleep 300 &\necho $! > {trace}\nsleep 300\n"),
        )
        .expect("script ecrit");
        let mut enfant = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(". {script}"))
            .process_group(0)
            .spawn()
            .expect("spawn");
        let resultat = attendre(&mut enfant, 400);
        assert!(resultat.is_err(), "delai depasse attendu");
        let pid_descendance = std::fs::read_to_string(&trace).expect("pid de descendance ecrit");
        let pid: u32 = pid_descendance.trim().parse().expect("pid numerique");
        let mut mort = false;
        for _ in 0..15 {
            if !vivant(pid) {
                mort = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(mort, "la descendance {pid} doit etre morte avec le groupe");
    }

    /// Vivant = présent dans /proc ET pas en état zombie : dans un
    /// container, l'init du namespace récolte les orphelins tard, un
    /// zombie tué par le SIGKILL ne prouve pas une survie.
    fn vivant(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(s) => match s
                .rsplit_once(')')
                .map(|(_, reste)| reste.trim().chars().next())
            {
                Some(Some('Z')) | Some(Some('X')) => false,
                Some(Some(_)) => true,
                _ => false,
            },
            Err(_) => false,
        }
    }
}
