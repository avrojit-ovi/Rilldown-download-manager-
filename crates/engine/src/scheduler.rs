//! Dynamically resizable concurrency gates.
//!
//! The engine needs two levels of admission control: a global limit and one
//! limit per queue. Tokio's `Semaphore` cannot be shrunk once permits are
//! handed out, so the gate below tracks its own counter and wakes waiters when
//! the limit changes.

use std::sync::Mutex;

use tokio::sync::Notify;

#[derive(Debug)]
struct GateState {
    active: u32,
    /// Zero means "no limit of its own"; the global gate still applies.
    limit: u32,
}

/// Counting gate with a runtime-adjustable limit.
#[derive(Debug)]
pub struct ConcurrencyGate {
    state: Mutex<GateState>,
    notify: Notify,
}

impl ConcurrencyGate {
    /// Create a gate with an initial limit. Zero disables this gate.
    #[must_use]
    pub fn new(limit: u32) -> Self {
        Self {
            state: Mutex::new(GateState { active: 0, limit }),
            notify: Notify::new(),
        }
    }

    /// Current limit.
    #[must_use]
    pub fn limit(&self) -> u32 {
        self.lock().limit
    }

    /// Change the limit and wake every waiter so they can re-check.
    pub fn set_limit(&self, limit: u32) {
        self.lock().limit = limit;
        self.notify.notify_waiters();
    }

    /// Number of currently admitted holders.
    #[must_use]
    pub fn active(&self) -> u32 {
        self.lock().active
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn try_enter(&self) -> bool {
        let mut state = self.lock();
        if state.limit == 0 || state.active < state.limit {
            state.active = state.active.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Wait for a slot. The returned permit releases it on drop.
    pub async fn acquire(&self) -> GatePermit<'_> {
        loop {
            // Register interest before checking so a release between the check
            // and the await cannot be missed.
            let notified = self.notify.notified();
            if self.try_enter() {
                return GatePermit { gate: self };
            }
            notified.await;
        }
    }

    fn release(&self) {
        let mut state = self.lock();
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.notify.notify_waiters();
    }
}

/// RAII slot held for the duration of a transfer.
#[derive(Debug)]
pub struct GatePermit<'a> {
    gate: &'a ConcurrencyGate,
}

impl Drop for GatePermit<'_> {
    fn drop(&mut self) {
        self.gate.release();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn enforces_the_limit_and_releases_slots() {
        let gate = ConcurrencyGate::new(2);
        let first = gate.acquire().await;
        let second = gate.acquire().await;
        assert_eq!(gate.active(), 2);

        let third = {
            let gate = &gate;
            tokio::time::timeout(Duration::from_millis(50), gate.acquire()).await
        };
        assert!(third.is_err(), "third waiter must block at the limit");

        drop(first);
        let third = tokio::time::timeout(Duration::from_secs(1), gate.acquire())
            .await
            .expect("a released slot wakes the waiter");
        assert_eq!(gate.active(), 2);
        drop(second);
        drop(third);
        assert_eq!(gate.active(), 0);
    }

    #[tokio::test]
    async fn raising_the_limit_wakes_waiters() {
        let gate = Arc::new(ConcurrencyGate::new(0));
        gate.set_limit(1);
        let held = gate.acquire().await;
        let waiting = {
            let gate = gate.clone();
            tokio::spawn(async move {
                let permit = gate.acquire().await;
                drop(permit);
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        gate.set_limit(2);
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("raising the limit wakes waiters")
            .unwrap();
        drop(held);
    }

    #[tokio::test]
    async fn zero_means_unlimited() {
        let gate = ConcurrencyGate::new(0);
        let _one = gate.acquire().await;
        let _two = gate.acquire().await;
        let _three = gate.acquire().await;
        assert_eq!(gate.active(), 3);
    }
}
