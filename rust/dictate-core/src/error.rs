use std::fmt;

/// A failure carrying a message fit to show the user. FR-7.4 wants the API's
/// own words in it, so conversions keep the whole source chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // reqwest's own Display is "error sending request for url (…)"; the
        // reason — timeout, DNS, TLS — is further down the chain.
        use std::error::Error as _;

        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(inner) = source {
            message.push_str(": ");
            message.push_str(&inner.to_string());
            source = inner.source();
        }
        Self::new(message)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}
