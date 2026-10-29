//! Media grabber errors.

/// Everything that can fail while locating tools, probing, or grabbing media.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("HTTP {status} while fetching {url}")]
    Http { status: u16, url: String },

    #[error("{0}")]
    MissingTool(String),

    #[error("{tool} failed: {message}")]
    ToolFailed { tool: String, message: String },

    #[error("the media grab was cancelled")]
    Cancelled,

    #[error("could not understand the media response: {0}")]
    Parse(String),

    #[error("{0}")]
    Unsupported(String),
}

/// Result alias for media operations.
pub type MediaResult<T> = Result<T, MediaError>;
