//! Persisted desktop preferences.
//!
//! Settings live in the engine's SQLite database under one JSON key so they
//! survive upgrades and stay next to download history. The shell applies the
//! concurrency and throughput limits to the running engine after every save.

use std::path::PathBuf;

use rilldown_engine::{Engine, EngineConfig, EngineResult};
use serde::{Deserialize, Serialize};

const SETTINGS_KEY: &str = "app.settings";

/// User-editable application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub download_dir: PathBuf,
    pub connections: u8,
    pub max_concurrent: u32,
    pub speed_limit_bps: Option<u64>,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
    pub clipboard_monitor: bool,
    pub close_to_tray: bool,
    pub notifications: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::new(),
            connections: rilldown_engine::branding::DEFAULT_CONNECTIONS,
            max_concurrent: 3,
            speed_limit_bps: None,
            proxy: None,
            user_agent: None,
            clipboard_monitor: false,
            close_to_tray: true,
            notifications: true,
        }
    }
}

impl AppSettings {
    /// Defaults for a fresh install, rooted at the engine's directories.
    #[must_use]
    pub fn from_config(config: &EngineConfig) -> Self {
        Self {
            download_dir: config.download_dir.clone(),
            max_concurrent: u32::try_from(config.max_concurrent_downloads).unwrap_or(3),
            speed_limit_bps: config.global_speed_limit_bps,
            ..Self::default()
        }
    }

    /// Load saved settings, filling in fields added by later versions.
    #[must_use]
    pub fn load(engine: &Engine) -> Self {
        let fallback = Self::from_config(engine.config());
        let Ok(Some(raw)) = engine.get_setting(SETTINGS_KEY) else {
            return fallback;
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(mut settings) => {
                if settings.download_dir.as_os_str().is_empty() {
                    settings.download_dir = fallback.download_dir;
                }
                settings.validate();
                settings
            }
            Err(error) => {
                tracing::warn!(%error, "ignoring unreadable saved settings");
                fallback
            }
        }
    }

    /// Persist settings.
    pub fn save(&self, engine: &Engine) -> EngineResult<()> {
        let encoded = serde_json::to_string(self)?;
        engine.set_setting(SETTINGS_KEY, &encoded)
    }

    /// Clamp values that come from a JSON file or the UI.
    pub fn validate(&mut self) {
        self.connections = self.connections.clamp(1, 32);
        self.max_concurrent = self.max_concurrent.clamp(1, 32);
        if self.speed_limit_bps == Some(0) {
            self.speed_limit_bps = None;
        }
        if self.download_dir.as_os_str().is_empty() {
            if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
            {
                self.download_dir = PathBuf::from(home).join("Downloads");
            } else {
                self.download_dir = PathBuf::from(".");
            }
        }
        self.proxy = clean_optional(self.proxy.take());
        self.user_agent = clean_optional(self.user_agent.take());
    }

    /// Apply limits to the running engine.
    pub fn apply(&self, engine: &Engine) {
        engine.set_max_concurrent(self.max_concurrent);
        engine.set_global_speed_limit(self.speed_limit_bps);
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_engine_config() {
        let config = EngineConfig::new("/tmp/rilldown-data", "/tmp/rilldown-downloads");
        let settings = AppSettings::from_config(&config);
        assert_eq!(
            settings.download_dir,
            PathBuf::from("/tmp/rilldown-downloads")
        );
        assert_eq!(settings.max_concurrent, 3);
        assert_eq!(settings.connections, 8);
    }

    #[test]
    fn validation_clamps_and_trims() {
        let mut settings = AppSettings {
            connections: 0,
            max_concurrent: 99,
            speed_limit_bps: Some(0),
            proxy: Some("   ".to_string()),
            ..AppSettings::default()
        };
        settings.validate();
        assert_eq!(settings.connections, 1);
        assert_eq!(settings.max_concurrent, 32);
        assert_eq!(settings.speed_limit_bps, None);
        assert_eq!(settings.proxy, None);
    }

    #[test]
    fn settings_round_trip_through_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let config = EngineConfig::new(directory.path().join("data"), directory.path().join("dl"));
        let engine = Engine::new(config.clone()).unwrap();
        let mut settings = AppSettings::from_config(&config);
        settings.max_concurrent = 7;
        settings.clipboard_monitor = true;
        settings.save(&engine).unwrap();

        let loaded = AppSettings::load(&engine);
        assert_eq!(loaded.max_concurrent, 7);
        assert!(loaded.clipboard_monitor);
    }
}
