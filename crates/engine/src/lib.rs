//! Rilldown's download engine.
//!
//! The engine is deliberately independent of Tauri and any UI toolkit. It owns
//! download state, persistence, transfer workers, queues, and the event stream.
//! A desktop shell, a CLI, or a test harness can all drive the same API.

pub mod branding;
pub mod category;
pub mod checksum;
pub mod config;
pub mod db;
pub mod engine;
pub mod error;
pub mod filename;
pub mod http;
pub mod scheduler;
pub mod segments;
pub mod throttle;
pub mod types;

pub use category::{Category, classify};
pub use config::{EngineConfig, RetryConfig};
pub use db::Store;
pub use engine::Engine;
pub use error::{EngineError, EngineResult};
pub use scheduler::ConcurrencyGate;
pub use segments::{SegmentBoard, plan_segments};
pub use throttle::RateLimiter;
pub use types::{
    Checksum, ChecksumAlgorithm, DEFAULT_QUEUE_ID, DownloadFilter, DownloadId, DownloadProgress,
    DownloadRecord, DownloadSpec, DownloadState, EngineEvent, ProbeInfo, Queue, Segment,
    SegmentProgress, SegmentState,
};
pub use url::Url;
