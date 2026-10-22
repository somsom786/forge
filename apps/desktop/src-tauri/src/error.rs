use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("git error: {0}")]
    Git(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("path rejected: {0}")]
    PathRejected(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("{0}")]
    Other(String),
}

impl Serialize for ForgeError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ForgeError>;
