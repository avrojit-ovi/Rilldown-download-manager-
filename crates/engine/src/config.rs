//! Engine configuration and retry policy.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};

/// Capped exponential backoff with jitter.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 500,
            max_delay_ms: 30_000,
        }
    }
}

impl RetryConfig {
    /// Delay before a retry. `attempt` starts at 1.
    #[must_use]
    pub fn delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(10);
        let base = self
            .base_delay_ms
            .saturating_mul(1_u64 << exponent)
            .min(self.max_delay_ms);
        let jitter_window = (base / 4).max(1);
        let jitter = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::from(elapsed.subsec_nanos()) % jitter_window
            });
        Duration::from_millis(base.saturating_add(jitter))
    }
}

/// Everything the engine needs to locate its state and defaults.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
    pub database_path: PathBuf,
    pub max_concurrent_downloads: usize,
    /// Engine-wide throughput ceiling. `None` means unlimited.
    pub global_speed_limit_bps: Option<u64>,
    pub retry: RetryConfig,
}

impl EngineConfig {
    /// Build a configuration rooted at an explicit data and download directory.
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>, download_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            database_path: data_dir.join("rilldown.db"),
            data_dir,
            download_dir: download_dir.into(),
            max_concurrent_downloads: 3,
            global_speed_limit_bps: None,
            retry: RetryConfig::default(),
        }
    }

    /// Per-user defaults for the CLI and fallback desktop launches.
    pub fn platform_default() -> EngineResult<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| EngineError::Other("cannot determine the user home directory".into()))?;
        let home = PathBuf::from(home);

        let data_root = if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Application Support")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map_or_else(|| home.join(".local").join("share"), PathBuf::from)
        };

        Ok(Self::new(
            data_root.join("rilldown"),
            home.join("Downloads"),
        ))
    }

    /// Create the data and download directories.
    pub fn ensure_directories(&self) -> EngineResult<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        if let Some(parent) = self.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&self.download_dir)?;
        Ok(())
    }

    /// Part-file path for a download, kept beside the destination so the final
    /// rename stays on the same filesystem.
    #[must_use]
    pub fn part_path(&self, save_dir: &Path, id: crate::types::DownloadId) -> PathBuf {
        save_dir.join(".rilldown").join(format!("{id}.part"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        let config = RetryConfig {
            max_attempts: 8,
            base_delay_ms: 100,
            max_delay_ms: 1000,
        };
        for attempt in 1..8 {
            let delay = config.delay(attempt).as_millis();
            assert!(delay >= 100, "attempt {attempt}: {delay}");
            assert!(delay <= 1250, "attempt {attempt}: {delay}");
        }
        assert!(config.delay(1) < config.delay(4));
        assert!(config.delay(10).as_millis() <= 1250);
    }
}
