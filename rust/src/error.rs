use std::fmt;

/// Failure from detection or packaging, with a message safe to show in the app.
#[derive(Debug)]
pub struct PackagerError {
    message: String,
}

impl PackagerError {
    pub fn msg(message: impl AsRef<str>) -> Self {
        Self {
            message: message.as_ref().to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PackagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PackagerError {}

impl From<std::io::Error> for PackagerError {
    fn from(value: std::io::Error) -> Self {
        Self::msg(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PackagerError>;
