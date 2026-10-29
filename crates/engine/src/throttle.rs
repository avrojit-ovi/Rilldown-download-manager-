//! Token-bucket throughput limiting.
//!
//! Each bucket is shared by every connection of one download (and, for the
//! global limiter, by every download). Callers reserve bytes before writing
//! them to disk; the bucket returns how long that reservation must sleep. The
//! bucket can go into debt, which smooths bursts instead of rejecting them.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Burst allowance is this fraction of the configured rate, with a floor.
const BURST_DIVISOR: u64 = 4;
const MIN_BURST_BYTES: f64 = 32.0 * 1024.0;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// A single rate limit, in bytes per second.
#[derive(Debug)]
pub struct RateLimiter {
    rate: f64,
    capacity: f64,
    bucket: Mutex<Bucket>,
}

impl RateLimiter {
    /// Build a limiter. A zero or nonsensical rate yields `None`.
    #[must_use]
    pub fn new(bytes_per_second: u64) -> Option<Self> {
        if bytes_per_second == 0 {
            return None;
        }
        let rate = bytes_per_second as f64;
        let capacity = (rate / BURST_DIVISOR as f64).max(MIN_BURST_BYTES);
        Some(Self {
            rate,
            capacity,
            bucket: Mutex::new(Bucket {
                tokens: capacity,
                last: Instant::now(),
            }),
        })
    }

    /// Configured rate.
    #[must_use]
    pub fn rate(&self) -> u64 {
        // Rates originate from u64 and round-trip through f64.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rate = self.rate.round() as u64;
        rate
    }

    /// Reserve `bytes`, sleeping until the bucket can afford them.
    pub async fn acquire(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let wait = self.reserve(bytes);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    fn reserve(&self, bytes: u64) -> Duration {
        #[allow(clippy::cast_precision_loss)]
        let requested = bytes as f64;
        let mut bucket = self
            .bucket
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.last = now;
        bucket.tokens = (bucket.tokens + elapsed * self.rate).min(self.capacity);
        bucket.tokens -= requested;
        if bucket.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-bucket.tokens / self.rate)
        }
    }
}

/// Combined per-download and global limiter.
#[derive(Debug, Clone, Default)]
pub struct Throttle {
    local: Option<std::sync::Arc<RateLimiter>>,
    global: Option<std::sync::Arc<RateLimiter>>,
}

impl Throttle {
    /// Combine a per-download rate with the engine-wide rate.
    #[must_use]
    pub fn new(local_bps: Option<u64>, global: Option<std::sync::Arc<RateLimiter>>) -> Self {
        Self {
            local: local_bps
                .and_then(RateLimiter::new)
                .map(std::sync::Arc::new),
            global,
        }
    }

    /// Whether any limit is active.
    #[must_use]
    pub fn is_limited(&self) -> bool {
        self.local.is_some() || self.global.is_some()
    }

    /// Reserve bytes against every active limit.
    pub async fn acquire(&self, bytes: u64) {
        if let Some(local) = &self.local {
            local.acquire(bytes).await;
        }
        if let Some(global) = &self.global {
            global.acquire(bytes).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[tokio::test]
    async fn an_unlimited_throttle_returns_immediately() {
        let throttle = Throttle::default();
        assert!(!throttle.is_limited());
        let start = Instant::now();
        throttle.acquire(8 * 1024 * 1024).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn throttle_delays_large_reservations() {
        let throttle = Throttle::new(Some(64 * 1024), None);
        assert!(throttle.is_limited());
        // The bucket starts full; drain it, then measure the debt sleep.
        throttle.acquire(64 * 1024).await;
        let start = Instant::now();
        throttle.acquire(64 * 1024).await;
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(700),
            "expected roughly a second of debt, waited {elapsed:?}"
        );
    }

    #[test]
    fn zero_rate_disables_the_limiter() {
        assert!(RateLimiter::new(0).is_none());
        assert_eq!(RateLimiter::new(1234).unwrap().rate(), 1234);
    }
}
