pub mod exec;
pub mod webhook;

use std::io::Write;

use crate::config::{Action, Config, Regle, Severite, TypeAction};
use crate::error::Result;
use crate::eval::Agir;
use crate::log::{Level, Logger};

pub struct ContexteAction {
    pub dry_run: bool,
    pub hostname: String,
}

pub struct Variables {
    pub nom: String,
    pub severite: &'static str,
    pub valeur: String,
    pub seuil: String,
    pub hostname: String,
    pub horodatage: String,
}

pub fn severite_texte(severite: Severite) -> &'static str {
    match severite {
        Severite::Info => "info",
        Severite::Warning => "warning",
        Severite::Critical => "critical",
    }
}

pub fn variables(regle: &Regle, valeur: f64, hostname: &str) -> Variables {
    Variables {
        nom: regle.nom.clone(),
        severite: severite_texte(regle.severite),
        valeur: format!("{valeur}"),
        seuil: format!("{}", regle.condition.valeur),
        hostname: hostname.to_string(),
        horodatage: crate::time::now_timestamp_secondes(),
    }
}

pub fn template_applique(src: &str, v: &Variables) -> String {
    src.replace("{{.RuleName}}", &v.nom)
        .replace("{{.Severity}}", v.severite)
        .replace("{{.Value}}", &v.valeur)
        .replace("{{.Threshold}}", &v.seuil)
        .replace("{{.Hostname}}", &v.hostname)
        .replace("{{.Timestamp}}", &v.horodatage)
}

pub struct ActionCours<'a> {
    pub act: &'a Action,
    pub regle: &'a Regle,
    pub valeur: f64,
}

pub fn executer<W: Write>(
    ac: &ActionCours,
    journal: &mut Logger<W>,
    cfg: &Config,
    ctx: &ContexteAction,
) -> Result<()> {
    match ac.act.kind {
        TypeAction::Log => log_action(ac, journal, ctx),
        _ if ctx.dry_run => dry_run(ac, journal),
        TypeAction::Webhook => webhook::envoit(ac, cfg, ctx),
        TypeAction::Exec => exec::exec(ac, journal, cfg, ctx),
    }
}

pub fn niveau_action(act: &Action, regle: &Regle) -> Level {
    match act.niveau_log {
        Some(niveau) => niveau,
        None => match regle.severite {
            Severite::Info => Level::Info,
            Severite::Critical => Level::Error,
            Severite::Warning => Level::Warn,
        },
    }
}

pub fn log_action<W: Write>(
    ac: &ActionCours,
    journal: &mut Logger<W>,
    ctx: &ContexteAction,
) -> Result<()> {
    let source = ac.act.message.as_deref().unwrap_or("regle déclenchée");
    let v = variables(ac.regle, ac.valeur, &ctx.hostname);
    journal.write(
        niveau_action(ac.act, ac.regle),
        &template_applique(source, &v),
    );
    Ok(())
}

pub fn dry_run<W: Write>(ac: &ActionCours, journal: &mut Logger<W>) -> Result<()> {
    match ac.act.kind {
        TypeAction::Webhook => journal.write(Level::Info, "dry-run : webhook ignore"),
        TypeAction::Exec => journal.write(Level::Info, "dry-run : exec ignore"),
        _ => {}
    }
    Ok(())
}

pub struct Lanceur<'a, W: Write> {
    pub journal: &'a mut Logger<W>,
    pub cfg: &'a Config,
    pub ctx: &'a ContexteAction,
}

impl<W: Write> Agir for Lanceur<'_, W> {
    fn agir(&mut self, regle: &Regle, valeur: f64) {
        for act in &regle.actions {
            let ac = ActionCours { act, regle, valeur };
            let _ = executer(&ac, self.journal, self.cfg, self.ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severites_texte() {
        assert_eq!(severite_texte(Severite::Info), "info");
        assert_eq!(severite_texte(Severite::Warning), "warning");
        assert_eq!(severite_texte(Severite::Critical), "critical");
    }

    #[test]
    fn template_jetons() {
        let v = Variables {
            nom: "essai".to_string(),
            severite: "critical",
            valeur: "75.5".to_string(),
            seuil: "50".to_string(),
            hostname: "hote-local".to_string(),
            horodatage: "2026-11-21T20:00:00Z".to_string(),
        };
        let message = template_applique(
            "{{.RuleName}} sur {{.Hostname}} : {{.Value}}/{{.Threshold}} ({{.Severity}}) a {{.Timestamp}}",
            &v,
        );
        assert!(message.contains("essai sur hote-local"));
        assert!(message.contains("75.5/50"));
        assert!(message.contains("(critical)"));
        assert!(message.contains("a 2026-11-21T20:00:00Z"));
        assert!(!message.contains("{{."));

        let conserve = template_applique("avant {{.Inconnue}} apres", &v);
        assert!(conserve.contains("{{.Inconnue}}"), "jeton inconnu conservé");
    }

    #[test]
    fn valeur_formatee() {
        let v = variables_seuil(75.5, 50.0);
        assert_eq!(v.valeur, "75.5");
        assert_eq!(v.seuil, "50");
        let v = variables_seuil(80.0, 0.42);
        assert_eq!(v.valeur, "80");
        assert_eq!(v.seuil, "0.42");
    }

    fn variables_seuil(valeur: f64, seuil: f64) -> Variables {
        let regle = Regle {
            nom: "essai".to_string(),
            severite: Severite::Critical,
            condition: crate::config::Condition {
                metrique: None,
                montage: None,
                operateur: None,
                valeur: seuil,
            },
            pour: 0,
            cooldown: 0,
            actions: Vec::new(),
        };
        variables(&regle, valeur, "hote-local")
    }
}
