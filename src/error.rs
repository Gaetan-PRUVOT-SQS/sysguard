use std::fmt;

#[derive(Debug, PartialEq)]
pub struct Error(String);

impl Error {
    pub fn new<S: Into<String>>(message: S) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_conserved() {
        let e = Error::new("configuration invalide : test");
        assert_eq!(e.to_string(), "configuration invalide : test");
    }

    #[test]
    fn error_trait_available() {
        let boxed: Box<dyn std::error::Error> = Box::new(Error::new("x"));
        assert_eq!(boxed.to_string(), "x");
    }
}
