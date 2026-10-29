//! Engine error model.
//!
//! Errors are small, typed, and serializable where they cross a UI boundary.
//! User-facing messages are produced by the shell; the variants carry enough
//! structure to localize them.

use crate::types::DownloadId;

/// Result alias used throughout the engine.
pub type EngineResult<T> = Result<T, EngineError>;

/// Everything that can go wrong inside the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("data serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("download not found: {0}")]
    NotFound(DownloadId),

    #[error("the server does not support range requests")]
    RangesUnsupported,

    #[error("the server returned HTTP {status} for {url}")]
    HttpStatus { status: u16, url: String },

    #[error("the server changed the file while it was being downloaded")]
    ValidatorChanged,

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("invalid filename: {0}")]
    InvalidFilename(String),

    #[error("the download cannot be resumed")]
    NotResumable,

    #[error("the connection closed before the byte range completed")]
    IncompleteTransfer,

    #[error("operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl EngineError {
    /// Stable machine-readable code for the UI and native protocol.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl(_) => "invalid_url",
            Self::UnsupportedScheme(_) => "unsupported_scheme",
            Self::Network(_) => "network",
            Self::Io(_) => "io",
            Self::Database(_) => "database",
            Self::Serialization(_) => "serialization",
            Self::NotFound(_) => "not_found",
            Self::RangesUnsupported => "ranges_unsupported",
            Self::HttpStatus { .. } => "http_status",
            Self::ValidatorChanged => "validator_changed",
            Self::ChecksumMismatch { .. } => "checksum_mismatch",
            Self::InvalidFilename(_) => "invalid_filename",
            Self::NotResumable => "not_resumable",
            Self::IncompleteTransfer => "incomplete_transfer",
            Self::Cancelled => "cancelled",
            Self::Other(_) => "other",
        }
    }

    /// Whether retrying the operation could plausibly succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::IncompleteTransfer => true,
            Self::HttpStatus { status, .. } => {
                matches!(status, 408 | 425 | 429 | 500..=599)
            }
            _ => false,
        }
    }
}
