//! Serializable contracts shared by the engine, desktop shell, CLI, and native host.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::branding::{DEFAULT_CONNECTIONS, MAX_CONNECTIONS};
use crate::error::{EngineError, EngineResult};
use crate::filename::{filename_from_url, sanitize_filename};

/// Opaque, stable identifier for a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub Uuid);

impl DownloadId {
    /// Generate a fresh identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for DownloadId {
    type Err = EngineError;

    fn from_str(value: &str) -> EngineResult<Self> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|error| EngineError::Other(format!("invalid download id: {error}")))
    }
}

/// Lifecycle state of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Probing,
    Downloading,
    Paused,
    Merging,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl DownloadState {
    /// Persistence and protocol representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Merging => "merging",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether no further work will happen without an explicit user action.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Whether the download currently owns network resources.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Probing | Self::Downloading | Self::Merging | Self::Verifying
        )
    }
}

impl fmt::Display for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DownloadState {
    type Err = EngineError;

    fn from_str(value: &str) -> EngineResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "probing" => Ok(Self::Probing),
            "downloading" => Ok(Self::Downloading),
            "paused" => Ok(Self::Paused),
            "merging" => Ok(Self::Merging),
            "verifying" => Ok(Self::Verifying),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(EngineError::Other(format!(
                "unknown download state: {other}"
            ))),
        }
    }
}

/// Checksum algorithms supported for post-download verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
}

impl ChecksumAlgorithm {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }
}

impl fmt::Display for ChecksumAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ChecksumAlgorithm {
    type Err = EngineError;

    fn from_str(value: &str) -> EngineResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "md5" => Ok(Self::Md5),
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            other => Err(EngineError::Other(format!(
                "unknown checksum algorithm: {other}"
            ))),
        }
    }
}

/// Optional expected digest for a completed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checksum {
    pub algorithm: ChecksumAlgorithm,
    pub expected_hex: String,
}

impl Checksum {
    /// Normalize and validate the expected digest length.
    pub fn validate(&self) -> EngineResult<()> {
        let normalized = self.expected_hex.trim().to_ascii_lowercase();
        let expected_len = match self.algorithm {
            ChecksumAlgorithm::Md5 => 32,
            ChecksumAlgorithm::Sha1 => 40,
            ChecksumAlgorithm::Sha256 => 64,
        };
        if normalized.len() != expected_len || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(EngineError::Other(format!(
                "invalid {} digest",
                self.algorithm
            )));
        }
        Ok(())
    }
}

/// Everything needed to enqueue a download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSpec {
    pub url: Url,
    #[serde(default)]
    pub mirrors: Vec<Url>,
    pub save_dir: PathBuf,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default = "default_connections")]
    pub connections: u8,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// Proxy URL, for example `socks5://127.0.0.1:1080`.
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub speed_limit_bps: Option<u64>,
    #[serde(default)]
    pub checksum: Option<Checksum>,
    #[serde(default)]
    pub queue_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

const fn default_connections() -> u8 {
    DEFAULT_CONNECTIONS
}

impl DownloadSpec {
    /// Build a spec with conservative defaults.
    #[must_use]
    pub fn new(url: Url, save_dir: PathBuf) -> Self {
        Self {
            url,
            mirrors: Vec::new(),
            save_dir,
            filename: None,
            connections: DEFAULT_CONNECTIONS,
            headers: BTreeMap::new(),
            cookies: None,
            referrer: None,
            user_agent: None,
            proxy: None,
            speed_limit_bps: None,
            checksum: None,
            queue_id: None,
            category: None,
        }
    }

    /// Validate and clamp user-provided values.
    pub fn normalize(&mut self) -> EngineResult<()> {
        match self.url.scheme() {
            "http" | "https" => {}
            scheme => return Err(EngineError::UnsupportedScheme(scheme.to_string())),
        }
        if self.url.host_str().is_none() {
            return Err(EngineError::InvalidUrl(self.url.to_string()));
        }
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        if let Some(limit) = self.speed_limit_bps {
            if limit == 0 {
                self.speed_limit_bps = None;
            }
        }
        if let Some(checksum) = &self.checksum {
            checksum.validate()?;
        }
        if let Some(filename) = &self.filename {
            let clean = sanitize_filename(filename);
            if clean.is_empty() {
                return Err(EngineError::InvalidFilename(filename.clone()));
            }
            self.filename = Some(clean);
        }
        Ok(())
    }

    /// Create the initial database record for this spec.
    #[must_use]
    pub fn into_record(self, id: DownloadId) -> DownloadRecord {
        let now = Utc::now();
        let filename = self
            .filename
            .clone()
            .or_else(|| filename_from_url(&self.url))
            .unwrap_or_else(|| "download".to_string());
        DownloadRecord {
            id,
            url: self.url,
            final_url: None,
            save_dir: self.save_dir,
            filename: sanitize_filename(&filename),
            custom_filename: self.filename.is_some(),
            state: DownloadState::Queued,
            total_bytes: None,
            downloaded_bytes: 0,
            resumable: false,
            etag: None,
            last_modified: None,
            content_type: None,
            error: None,
            checksum: self.checksum,
            checksum_actual: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
            queue_id: self.queue_id,
            category: self.category,
            connections: self.connections,
            speed_limit_bps: self.speed_limit_bps,
            headers: self.headers,
            cookies: self.cookies,
            referrer: self.referrer,
            user_agent: self.user_agent,
            proxy: self.proxy,
            mirrors: self.mirrors,
        }
    }
}

/// Full persisted state of one download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub url: Url,
    pub final_url: Option<Url>,
    pub save_dir: PathBuf,
    pub filename: String,
    /// True when the user supplied the name and a server filename must not
    /// replace it during the initial probe.
    pub custom_filename: bool,
    pub state: DownloadState,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub error: Option<String>,
    pub checksum: Option<Checksum>,
    pub checksum_actual: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub queue_id: Option<String>,
    pub category: Option<String>,
    pub connections: u8,
    pub speed_limit_bps: Option<u64>,
    pub headers: BTreeMap<String, String>,
    pub cookies: Option<String>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub proxy: Option<String>,
    pub mirrors: Vec<Url>,
}

impl DownloadRecord {
    /// Absolute destination path, once the filename is known.
    #[must_use]
    pub fn destination(&self) -> PathBuf {
        self.save_dir.join(&self.filename)
    }

    /// Fraction complete in the range `0.0..=1.0`.
    ///
    /// This is a presentation value; the cast to `f64` is intentional.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn fraction(&self) -> Option<f64> {
        match self.total_bytes {
            Some(total) if total > 0 => {
                Some((self.downloaded_bytes as f64 / total as f64).min(1.0))
            }
            _ => None,
        }
    }
}

/// State of a single byte-range worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    Pending,
    Active,
    Complete,
}

impl SegmentState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Complete => "complete",
        }
    }
}

impl FromStr for SegmentState {
    type Err = EngineError;

    fn from_str(value: &str) -> EngineResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "complete" => Ok(Self::Complete),
            other => Err(EngineError::Other(format!(
                "unknown segment state: {other}"
            ))),
        }
    }
}

/// A half-open byte range assignment for one connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub index: u32,
    pub start_byte: u64,
    /// Inclusive end offset.
    pub end_byte: u64,
    pub downloaded: u64,
    pub state: SegmentState,
}

impl Segment {
    /// Total bytes assigned to this segment.
    #[must_use]
    pub const fn len_bytes(self) -> u64 {
        self.end_byte.saturating_sub(self.start_byte) + 1
    }

    /// Bytes still to fetch.
    #[must_use]
    pub const fn remaining(self) -> u64 {
        self.len_bytes().saturating_sub(self.downloaded)
    }

    /// Next absolute offset to request.
    #[must_use]
    pub const fn next_offset(self) -> u64 {
        self.start_byte + self.downloaded
    }

    /// Whether the segment has received every assigned byte.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.downloaded >= self.len_bytes()
    }
}

/// Per-connection view for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentProgress {
    pub index: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded: u64,
    pub speed_bps: u64,
    pub active: bool,
}

/// Live progress snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub id: DownloadId,
    pub state: DownloadState,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
    pub connections: Vec<SegmentProgress>,
}

impl DownloadProgress {
    /// Build a progress snapshot from a persisted record.
    #[must_use]
    pub fn from_record(record: &DownloadRecord, speed_bps: u64) -> Self {
        let eta_seconds = match (record.total_bytes, speed_bps) {
            (Some(total), speed) if speed > 0 => {
                Some(total.saturating_sub(record.downloaded_bytes) / speed)
            }
            _ => None,
        };
        Self {
            id: record.id,
            state: record.state,
            downloaded_bytes: record.downloaded_bytes,
            total_bytes: record.total_bytes,
            speed_bps,
            eta_seconds,
            connections: Vec::new(),
        }
    }
}

/// Result of probing a remote resource before transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub final_url: Url,
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
    pub accept_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub filename: Option<String>,
}

/// Events emitted by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// Periodic progress snapshot.
    Progress { progress: DownloadProgress },
    /// A download moved to a new lifecycle state.
    StateChanged {
        id: DownloadId,
        state: DownloadState,
        error: Option<String>,
    },
    /// A download finished and its final file is available.
    Completed { id: DownloadId, path: PathBuf },
    /// Human-readable log line, already redacted.
    Log { level: String, message: String },
}

/// Identifier of the queue that always exists.
pub const DEFAULT_QUEUE_ID: &str = "main";

/// A named set of downloads with its own admission limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    pub id: String,
    pub name: String,
    /// Per-queue connection limit. Zero means "use only the global limit".
    pub max_concurrent: u32,
    #[serde(default)]
    pub speed_limit_bps: Option<u64>,
    pub created_at: DateTime<Utc>,
}

impl Queue {
    /// The built-in queue every download joins unless told otherwise.
    #[must_use]
    pub fn main() -> Self {
        Self {
            id: DEFAULT_QUEUE_ID.to_string(),
            name: "Main".to_string(),
            max_concurrent: 0,
            speed_limit_bps: None,
            created_at: Utc::now(),
        }
    }

    /// Trim and validate user-editable fields.
    pub fn normalize(&mut self) -> EngineResult<()> {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        if self.id.is_empty() || self.id.len() > 64 {
            return Err(EngineError::Other(
                "queue id must be 1-64 characters".into(),
            ));
        }
        if !self
            .id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err(EngineError::Other(
                "queue id may only contain letters, digits, '-' and '_'".into(),
            ));
        }
        if self.name.is_empty() || self.name.chars().count() > 80 {
            return Err(EngineError::Other(
                "queue name must be 1-80 characters".into(),
            ));
        }
        if self.max_concurrent > 64 {
            return Err(EngineError::Other(
                "queue concurrency cannot exceed 64".into(),
            ));
        }
        if self.speed_limit_bps == Some(0) {
            self.speed_limit_bps = None;
        }
        Ok(())
    }
}

/// Filter for listing downloads in the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadFilter {
    #[serde(default)]
    pub states: Vec<DownloadState>,
    #[serde(default)]
    pub queue_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
}

impl DownloadFilter {
    /// Whether a record matches this filter.
    #[must_use]
    pub fn matches(&self, record: &DownloadRecord) -> bool {
        if !self.states.is_empty() && !self.states.contains(&record.state) {
            return false;
        }
        if let Some(queue) = &self.queue_id {
            if record.queue_id.as_ref() != Some(queue) {
                return false;
            }
        }
        if let Some(category) = &self.category {
            if record.category.as_ref() != Some(category) {
                return false;
            }
        }
        if let Some(search) = &self.search {
            let needle = search.trim().to_ascii_lowercase();
            if !needle.is_empty()
                && !record.filename.to_ascii_lowercase().contains(&needle)
                && !record.url.as_str().to_ascii_lowercase().contains(&needle)
            {
                return false;
            }
        }
        true
    }
}
