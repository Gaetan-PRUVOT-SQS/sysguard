use std::io::Write;

use crate::error::{Error, Result};
use crate::time;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

pub fn level_from_text(text: &str) -> Result<Level> {
    match text {
        "debug" => Ok(Level::Debug),
        "info" => Ok(Level::Info),
        "warn" | "warning" => Ok(Level::Warn),
        "error" => Ok(Level::Error),
        _ => Err(Error::new(format!("niveau de journal inconnu : {text}"))),
    }
}

pub struct Logger<W: Write> {
    min: Level,
    sink: W,
}

impl<W: Write> Logger<W> {
    pub fn new(level_text: &str, sink: W) -> Result<Self> {
        let min = level_from_text(level_text)?;
        Ok(Self { min, sink })
    }

    pub fn contenu(self) -> W {
        self.sink
    }

    pub fn tampon(&mut self) -> &W {
        &self.sink
    }

    pub fn write(&mut self, level: Level, msg: &str) {
        if level < self.min {
            return;
        }
        let line = format!(
            "{{\"time\":\"{}\",\"level\":\"{}\",\"msg\":\"{}\"}}\n",
            time::now_timestamp(),
            level.name(),
            escape(msg),
        );
        let written = self.sink.write_all(line.as_bytes()).is_ok();
        if !written || self.sink.flush().is_err() {
            eprintln!("sysguard : ecriture du journal en echec");
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SinkQuiEchoue;

    impl Write for SinkQuiEchoue {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("flux casse"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flux casse"))
        }
    }

    #[test]
    fn accepted_levels() {
        assert_eq!(level_from_text("debug"), Ok(Level::Debug));
        assert_eq!(level_from_text("info"), Ok(Level::Info));
        assert_eq!(level_from_text("warning"), Ok(Level::Warn));
        assert_eq!(level_from_text("warn"), Ok(Level::Warn));
        assert_eq!(level_from_text("error"), Ok(Level::Error));
    }

    #[test]
    fn unknown_level_refused() {
        assert!(level_from_text("grave").is_err());
        assert!(level_from_text("").is_err());
        assert!(level_from_text("INFO").is_err());
    }

    #[test]
    fn json_line_written() {
        let mut logger = Logger::new("info", Vec::new()).expect("niveau valide");
        logger.write(Level::Info, "bonjour");
        let out = String::from_utf8(logger.contenu()).expect("utf-8");
        assert!(out.starts_with('{'));
        assert!(out.contains("\"time\":\"2"));
        assert!(out.contains("\"level\":\"INFO\""));
        assert!(out.contains("\"msg\":\"bonjour\""));
        assert!(out.ends_with("}\n"));
    }

    #[test]
    fn filtered_below_level() {
        let mut logger = Logger::new("warn", Vec::new()).expect("niveau valide");
        logger.write(Level::Info, "cache");
        logger.write(Level::Debug, "cache aussi");
        logger.write(Level::Warn, "garde");
        let out = String::from_utf8(logger.contenu()).expect("utf-8");
        assert!(!out.contains("cache"));
        assert!(out.contains("\"msg\":\"garde\""));
    }

    #[test]
    fn quotes_and_backslash_escaped() {
        let mut logger = Logger::new("error", Vec::new()).expect("niveau valide");
        logger.write(Level::Error, "cite \"guillemets\" et \\antislash");
        let out = String::from_utf8(logger.contenu()).expect("utf-8");
        assert!(out.contains("\\\"guillemets\\\""));
        assert!(out.contains("\\\\antislash"));
        assert!(!out.contains("\"msg\":\"guillemets"));
    }

    #[test]
    fn control_char_escaped() {
        let mut logger = Logger::new("info", Vec::new()).expect("niveau valide");
        logger.write(Level::Info, "sonne \u{7} fort");
        let out = String::from_utf8(logger.contenu()).expect("utf-8");
        assert!(out.contains("\\u0007"));
    }

    #[test]
    fn failing_sink_reported_without_panic() {
        let mut logger = Logger::new("info", SinkQuiEchoue).expect("niveau valide");
        logger.write(Level::Info, "quelconque");
    }
}
