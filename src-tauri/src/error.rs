use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Image error: {0}")]
    Image(String),

    // Domain-specific variants
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    #[error("Task not found: {0}")]
    TaskNotFound(String),
    #[error("Auth required: {0}")]
    AuthRequired(String),
    #[error("Invalid task type: {0}")]
    InvalidTaskType(String),
    #[error("Config missing: {0}")]
    ConfigMissing(String),
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("{0}")]
    Other(String),
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e.to_string())
    }
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Every `Error` can be turned into a `String` for Tauri command results.
impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.to_string()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
