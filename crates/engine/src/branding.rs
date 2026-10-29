//! Single source of truth for product identity and engine limits.

/// Human-readable application name.
pub const APP_NAME: &str = "Rilldown";

/// Reverse-DNS identifier used by the desktop shell and native host.
pub const APP_ID: &str = "app.rilldown.desktop";

/// Engine version, inherited from the crate manifest.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default outbound user agent. A download-specific override takes precedence.
pub const USER_AGENT: &str = concat!(
    "Rilldown/",
    env!("CARGO_PKG_VERSION"),
    " (+https://rilldown.com)"
);

/// Connections used when a caller does not specify a preference.
pub const DEFAULT_CONNECTIONS: u8 = 8;

/// Hard upper bound on concurrent connections for a single download.
pub const MAX_CONNECTIONS: u8 = 32;

/// Smallest segment worth creating. Avoids a swarm of tiny requests.
pub const MIN_SEGMENT_BYTES: u64 = 64 * 1024;

/// Native-messaging frame limit (Chrome's documented ceiling is 1 MiB).
pub const NATIVE_MESSAGE_MAX_BYTES: usize = 1024 * 1024;
