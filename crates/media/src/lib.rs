//! Media detection, tool management, and the yt-dlp based grabber.
//!
//! This crate never opens the download database. It answers three questions for
//! the shell and the browser extension: is this URL media, which formats does it
//! offer, and how do we fetch it with a supervised external process.

pub mod detect;
pub mod error;
pub mod grab;
pub mod tools;

pub use detect::{
    HlsVariant, MediaCandidate, StreamFormat, detect_format, is_media_url, parse_hls_master,
};
pub use error::{MediaError, MediaResult};
pub use grab::{GrabEvent, GrabOutcome, GrabRequest, MediaFormat, MediaInfo, grab, probe};
pub use tools::{Tool, ToolProgress, ToolStatus, Tools};
