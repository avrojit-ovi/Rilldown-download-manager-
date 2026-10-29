//! Download orchestration.
//!
//! One download owns a probe, a sparse `.part` file, a set of byte-range
//! workers, and a checkpoint task. Workers write only their own ranges and
//! periodically hand progress to SQLite. A final fsync, optional checksum, and
//! atomic rename make the completed file visible.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Client;
use reqwest::header::{CONTENT_ENCODING, CONTENT_RANGE, IF_RANGE, RANGE};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, broadcast};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::branding::MIN_SEGMENT_BYTES;
use crate::category;
use crate::checksum;
use crate::config::EngineConfig;
use crate::db::Store;
use crate::error::{EngineError, EngineResult};
use crate::filename::{ensure_extension, sanitize_filename, unique_path};
use crate::http;
use crate::scheduler::ConcurrencyGate;
use crate::segments::{SegmentBoard, plan_segments};
use crate::throttle::{RateLimiter, Throttle};
use crate::types::{
    DEFAULT_QUEUE_ID, DownloadFilter, DownloadId, DownloadProgress, DownloadRecord, DownloadSpec,
    DownloadState, EngineEvent, Queue, SegmentProgress, SegmentState,
};

const INTENT_NONE: u8 = 0;
const INTENT_PAUSE: u8 = 1;
const INTENT_CANCEL: u8 = 2;
const CHECKPOINT_INTERVAL: Duration = Duration::from_millis(400);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// Download engine facade. Cheap to clone; all clones share state.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    config: EngineConfig,
    events: broadcast::Sender<EngineEvent>,
    active: Mutex<HashMap<DownloadId, Active>>,
    /// Global admission gate for active transfers.
    gate: ConcurrencyGate,
    /// One admission gate per queue, keyed by queue id.
    queue_gates: StdMutex<HashMap<String, Arc<ConcurrencyGate>>>,
    /// Engine-wide ceiling in bytes per second; zero means unlimited.
    global_limit: AtomicU64,
    /// Shared token bucket for the global ceiling, rebuilt when it changes.
    global_limiter: StdMutex<Option<Arc<RateLimiter>>>,
}

struct Active {
    cancel: CancellationToken,
    intent: Arc<AtomicU8>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Open the database, recover interrupted states, and prepare directories.
    pub fn new(config: EngineConfig) -> EngineResult<Self> {
        config.ensure_directories()?;
        let store = Store::open(&config.database_path)?;
        store.recover_interrupted()?;
        store.ensure_default_queue()?;
        let (events, _) = broadcast::channel(512);
        let max_concurrent = u32::try_from(config.max_concurrent_downloads)
            .unwrap_or(3)
            .clamp(1, 32);
        let global_limit = config.global_speed_limit_bps.unwrap_or(0);
        let global_limiter = RateLimiter::new(global_limit).map(Arc::new);
        let mut queue_gates = HashMap::new();
        for queue in store.list_queues()? {
            queue_gates.insert(
                queue.id,
                Arc::new(ConcurrencyGate::new(queue.max_concurrent)),
            );
        }
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                config,
                events,
                active: Mutex::new(HashMap::new()),
                gate: ConcurrencyGate::new(max_concurrent),
                queue_gates: StdMutex::new(queue_gates),
                global_limit: AtomicU64::new(global_limit),
                global_limiter: StdMutex::new(global_limiter),
            }),
        })
    }

    /// Subscribe to progress and lifecycle events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    /// Current configuration.
    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.inner.config
    }

    /// Read a persisted shell setting.
    pub fn get_setting(&self, key: &str) -> EngineResult<Option<String>> {
        self.inner.store.get_setting(key)
    }

    /// Persist a shell setting.
    pub fn set_setting(&self, key: &str, value: &str) -> EngineResult<()> {
        self.inner.store.set_setting(key, value)
    }

    /// Queues, in creation order, always including the built-in queue.
    pub fn queues(&self) -> EngineResult<Vec<Queue>> {
        self.inner.store.list_queues()
    }

    /// Create or update a queue and apply its admission limit immediately.
    pub fn save_queue(&self, mut queue: Queue) -> EngineResult<Queue> {
        queue.normalize()?;
        self.inner.store.save_queue(&queue)?;
        let gate = {
            let mut gates = self
                .inner
                .queue_gates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gates
                .entry(queue.id.clone())
                .or_insert_with(|| Arc::new(ConcurrencyGate::new(queue.max_concurrent)))
                .clone()
        };
        gate.set_limit(queue.max_concurrent);
        Ok(queue)
    }

    /// Remove a queue, moving its downloads to the built-in queue.
    pub fn remove_queue(&self, id: &str) -> EngineResult<bool> {
        let removed = self.inner.store.delete_queue(id)?;
        if removed {
            self.inner
                .queue_gates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(id);
        }
        Ok(removed)
    }

    /// Global admission limit.
    #[must_use]
    pub fn max_concurrent(&self) -> u32 {
        self.inner.gate.limit()
    }

    /// Change the global admission limit; waiting transfers are woken.
    pub fn set_max_concurrent(&self, limit: u32) {
        self.inner.gate.set_limit(limit.clamp(1, 32));
    }

    /// Engine-wide throughput ceiling in bytes per second.
    #[must_use]
    pub fn global_speed_limit(&self) -> Option<u64> {
        match self.inner.global_limit.load(Ordering::Relaxed) {
            0 => None,
            value => Some(value),
        }
    }

    /// Set the engine-wide throughput ceiling, applied to transfers already running.
    pub fn set_global_speed_limit(&self, bytes_per_second: Option<u64>) {
        let value = bytes_per_second.unwrap_or(0);
        self.inner.global_limit.store(value, Ordering::Relaxed);
        *self
            .inner
            .global_limiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            RateLimiter::new(value).map(Arc::new);
    }

    /// Pause every queued or active transfer. Returns how many were paused.
    pub async fn pause_all(&self) -> EngineResult<usize> {
        let mut paused = 0;
        for record in self.inner.store.list_downloads()? {
            if record.state.is_terminal() || record.state == DownloadState::Paused {
                continue;
            }
            self.pause(record.id).await?;
            paused += 1;
        }
        Ok(paused)
    }

    /// Resume every paused transfer. Returns how many were resumed.
    pub async fn resume_all(&self) -> EngineResult<usize> {
        let mut resumed = 0;
        for record in self.inner.store.list_downloads()? {
            if record.state != DownloadState::Paused {
                continue;
            }
            self.resume(record.id).await?;
            resumed += 1;
        }
        Ok(resumed)
    }

    fn queue_gate(&self, id: &str) -> Arc<ConcurrencyGate> {
        let mut gates = self
            .inner
            .queue_gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        gates
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(ConcurrencyGate::new(0)))
            .clone()
    }

    fn global_throttle(&self) -> Option<Arc<RateLimiter>> {
        self.inner
            .global_limiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Read one download.
    pub fn get(&self, id: DownloadId) -> EngineResult<DownloadRecord> {
        self.inner
            .store
            .load_download(id)?
            .ok_or(EngineError::NotFound(id))
    }

    /// List downloads matching a filter.
    pub fn list(&self, filter: &DownloadFilter) -> EngineResult<Vec<DownloadRecord>> {
        self.inner.store.list_filtered(filter)
    }

    /// Insert a spec and start it.
    pub async fn add(&self, mut spec: DownloadSpec) -> EngineResult<DownloadRecord> {
        spec.normalize()?;
        let id = DownloadId::new();
        let record = spec.into_record(id);
        self.inner.store.save_download(&record)?;
        self.emit(EngineEvent::StateChanged {
            id,
            state: DownloadState::Queued,
            error: None,
        });
        self.start(id).await?;
        Ok(record)
    }

    /// Pause an active download or a queued one.
    pub async fn pause(&self, id: DownloadId) -> EngineResult<()> {
        let record = self.get(id)?;
        if record.state.is_terminal() || record.state == DownloadState::Paused {
            return Ok(());
        }
        match self.stop_active(id, INTENT_PAUSE).await {
            Some(task) => {
                let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
                Ok(())
            }
            None => self.set_state(id, DownloadState::Paused, None),
        }
    }

    /// Resume a paused or failed download.
    pub async fn resume(&self, id: DownloadId) -> EngineResult<()> {
        let mut record = self.get(id)?;
        if record.state == DownloadState::Completed {
            return Ok(());
        }
        if self.inner.active.lock().await.contains_key(&id) {
            return Ok(());
        }

        // A checksum failure leaves complete segments behind. Reusing them would
        // immediately fail again, so restart that download from zero.
        if record.state == DownloadState::Failed {
            let segments = self.inner.store.load_segments(id)?;
            if !segments.is_empty()
                && segments
                    .iter()
                    .all(|segment| segment.state == SegmentState::Complete)
            {
                self.inner.store.replace_segments(id, &[])?;
                self.cleanup_part(&record).await;
                record.downloaded_bytes = 0;
            }
        }

        record.state = DownloadState::Queued;
        record.error = None;
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.emit(EngineEvent::StateChanged {
            id,
            state: DownloadState::Queued,
            error: None,
        });
        self.start(id).await
    }

    /// Cancel a download and remove its partial data.
    pub async fn cancel(&self, id: DownloadId) -> EngineResult<()> {
        let record = self.get(id)?;
        if record.state == DownloadState::Cancelled {
            return Ok(());
        }
        if let Some(task) = self.stop_active(id, INTENT_CANCEL).await {
            let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
        } else {
            self.cleanup_part(&record).await;
            self.set_state(id, DownloadState::Cancelled, None)?;
        }
        Ok(())
    }

    /// Delete a record, its partial file, and optionally the completed file.
    pub async fn remove(&self, id: DownloadId, delete_file: bool) -> EngineResult<()> {
        let record = self.get(id)?;
        if let Some(task) = self.stop_active(id, INTENT_CANCEL).await {
            let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
        }
        self.cleanup_part(&record).await;
        if delete_file {
            let path = record.destination();
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(EngineError::Io(error)),
            }
        }
        self.inner.store.delete_download(id)?;
        Ok(())
    }

    /// Cancel every active transfer. Used on application shutdown.
    pub async fn shutdown(&self) {
        let mut active = self.inner.active.lock().await;
        let entries: Vec<Active> = active.drain().map(|(_, value)| value).collect();
        drop(active);
        for entry in entries {
            entry.intent.store(INTENT_CANCEL, Ordering::SeqCst);
            entry.cancel.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(5), entry.task).await;
        }
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.inner.events.send(event);
    }

    fn set_state(
        &self,
        id: DownloadId,
        state: DownloadState,
        error: Option<String>,
    ) -> EngineResult<()> {
        let Some(mut record) = self.inner.store.load_download(id)? else {
            return Ok(());
        };
        record.state = state;
        record.error = error;
        record.updated_at = Utc::now();
        if state == DownloadState::Completed {
            record.completed_at = Some(record.updated_at);
        }
        self.inner.store.save_download(&record)?;
        self.emit(EngineEvent::StateChanged {
            id,
            state,
            error: record.error.clone(),
        });
        Ok(())
    }

    async fn stop_active(&self, id: DownloadId, intent: u8) -> Option<JoinHandle<()>> {
        let entry = self.inner.active.lock().await.remove(&id)?;
        entry.intent.store(intent, Ordering::SeqCst);
        entry.cancel.cancel();
        Some(entry.task)
    }

    async fn start(&self, id: DownloadId) -> EngineResult<()> {
        let record = self.get(id)?;
        if record.state.is_terminal() {
            return Ok(());
        }

        // Wait for a previous task to finish before starting another one.
        let previous = self.inner.active.lock().await.remove(&id);
        if let Some(previous) = previous {
            let _ = previous.task.await;
        }

        let cancel = CancellationToken::new();
        let intent = Arc::new(AtomicU8::new(INTENT_NONE));
        let engine = self.clone();
        let task_cancel = cancel.clone();
        let task_intent = intent.clone();
        let task = tokio::spawn(async move {
            engine.run(id, task_cancel, task_intent).await;
        });

        self.inner.active.lock().await.insert(
            id,
            Active {
                cancel,
                intent,
                task,
            },
        );
        Ok(())
    }

    async fn run(&self, id: DownloadId, cancel: CancellationToken, intent: Arc<AtomicU8>) {
        let result = self.run_download(id, &cancel).await;
        match result {
            Ok(()) => {}
            Err(EngineError::Cancelled) => {
                let mode = intent.load(Ordering::SeqCst);
                if mode == INTENT_CANCEL {
                    if let Ok(record) = self.get(id) {
                        self.cleanup_part(&record).await;
                    }
                    let _ = self.set_state(id, DownloadState::Cancelled, None);
                } else {
                    if let Ok(record) = self.get(id) {
                        if !record.resumable {
                            self.cleanup_part(&record).await;
                        }
                    }
                    let _ = self.set_state(id, DownloadState::Paused, None);
                }
            }
            Err(error) => {
                if !matches!(error, EngineError::Cancelled) {
                    warn!(download = %id, %error, "download failed");
                    let _ = self.set_state(id, DownloadState::Failed, Some(error.to_string()));
                }
            }
        }
        self.inner.active.lock().await.remove(&id);
    }

    async fn run_download(&self, id: DownloadId, cancel: &CancellationToken) -> EngineResult<()> {
        let mut record = self.get(id)?;
        let queue_id = record
            .queue_id
            .clone()
            .unwrap_or_else(|| DEFAULT_QUEUE_ID.to_string());
        let queue_gate = self.queue_gate(&queue_id);

        // Global permits come first so a queue permit is never held while the
        // transfer waits for room that only a global slot can provide.
        let _global = tokio::select! {
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
            permit = self.inner.gate.acquire() => permit,
        };
        let _queue = tokio::select! {
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
            permit = queue_gate.acquire() => permit,
        };

        if matches!(
            record.state,
            DownloadState::Completed | DownloadState::Cancelled
        ) {
            return Ok(());
        }
        if record.state == DownloadState::Paused {
            return Err(EngineError::Cancelled);
        }
        let resuming = matches!(record.state, DownloadState::Paused | DownloadState::Failed);

        record.state = DownloadState::Probing;
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);

        let client = http::build_client(&record)?;
        let probe = tokio::select! {
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
            result = http::probe(&client, &record) => result?,
        };

        let restart = self.apply_probe(&mut record, probe)?;
        if restart {
            self.inner.store.replace_segments(id, &[])?;
            self.cleanup_part(&record).await;
            record.downloaded_bytes = 0;
        }

        if record.total_bytes == Some(0) {
            return self.finalize_empty(record).await;
        }

        let total = record.total_bytes;
        if let Some(total) = total {
            let available = fs2::available_space(&record.save_dir).unwrap_or(u64::MAX);
            let required = total.saturating_add(32 * 1024 * 1024);
            if available < required {
                return Err(EngineError::Other(format!(
                    "not enough free space: need at least {required} bytes, have {available}"
                )));
            }
        }

        let throttle = Arc::new(Throttle::new(
            record.speed_limit_bps,
            self.global_throttle(),
        ));
        if !record.resumable || total.is_none() {
            return self
                .run_single_stream(record, &client, cancel, throttle)
                .await;
        }

        self.run_segmented(record, &client, cancel, resuming, throttle)
            .await
    }

    /// Merge probe results into the record. Returns true if partial data must be
    /// discarded because the remote validator changed.
    fn apply_probe(
        &self,
        record: &mut DownloadRecord,
        probe: crate::types::ProbeInfo,
    ) -> EngineResult<bool> {
        let previous_etag = record.etag.clone();
        let previous_total = record.total_bytes;
        let restart = (previous_etag.is_some() && previous_etag != probe.etag)
            || (previous_total.is_some()
                && probe.content_length.is_some()
                && previous_total != probe.content_length);

        record.final_url = Some(probe.final_url);
        if !record.custom_filename {
            if let Some(filename) = probe.filename {
                record.filename = sanitize_filename(&filename);
            }
        }
        record.filename = ensure_extension(&record.filename, probe.content_type.as_deref());
        record.total_bytes = probe.content_length;
        record.resumable = probe.accept_ranges && probe.content_length.is_some();
        record.etag = probe.etag;
        record.last_modified = probe.last_modified;
        if record.category.is_none() {
            record.category = Some(
                category::classify(&record.filename, record.content_type.as_deref())
                    .id()
                    .to_string(),
            );
        }
        record.content_type = probe.content_type;
        record.error = None;
        record.updated_at = Utc::now();

        std::fs::create_dir_all(&record.save_dir)?;
        let destination = unique_path(&record.save_dir, &record.filename);
        if let Some(name) = destination.file_name().and_then(|name| name.to_str()) {
            record.filename = sanitize_filename(name);
        }

        self.inner.store.save_download(record)?;
        Ok(restart)
    }

    fn emit_state(&self, record: &DownloadRecord) {
        self.emit(EngineEvent::StateChanged {
            id: record.id,
            state: record.state,
            error: record.error.clone(),
        });
    }

    async fn run_segmented(
        &self,
        mut record: DownloadRecord,
        client: &Client,
        cancel: &CancellationToken,
        resuming: bool,
        throttle: Arc<Throttle>,
    ) -> EngineResult<()> {
        let total = record.total_bytes.ok_or(EngineError::RangesUnsupported)?;
        let part = self.inner.config.part_path(&record.save_dir, record.id);
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut segments = self.inner.store.load_segments(record.id)?;
        let downloaded_from_db: u64 = segments.iter().map(|segment| segment.downloaded).sum();
        let part_exists = tokio::fs::metadata(&part).await.is_ok();
        let resume = resuming && !segments.is_empty() && part_exists && downloaded_from_db <= total;

        if resume {
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&part)
                .await?;
            file.set_len(total).await?;
            for segment in &mut segments {
                if segment.state == SegmentState::Active {
                    segment.state = SegmentState::Pending;
                }
            }
            self.inner.store.replace_segments(record.id, &segments)?;
            record.downloaded_bytes = downloaded_from_db;
        } else {
            segments = plan_segments(total, record.connections);
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&part)
                .await?;
            file.set_len(total).await?;
            self.inner.store.replace_segments(record.id, &segments)?;
            record.downloaded_bytes = 0;
        }

        record.state = DownloadState::Downloading;
        record.error = None;
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);

        let board = Arc::new(SegmentBoard::new(segments, total));
        let ticker_cancel = CancellationToken::new();
        let ticker = self.spawn_ticker(record.id, board.clone(), ticker_cancel.clone());

        let record = Arc::new(record);
        let mut tasks = JoinSet::new();
        for segment in board.snapshot() {
            if segment.state == SegmentState::Complete {
                continue;
            }
            board.mark_active(segment.index);
            let engine = self.clone();
            let record = record.clone();
            let client = client.clone();
            let board = board.clone();
            let cancel = cancel.clone();
            let throttle = throttle.clone();
            tasks.spawn(async move {
                engine
                    .segment_worker(&record, &client, board, segment.index, &cancel, throttle)
                    .await
            });
        }

        let mut first_error: Option<EngineError> = None;
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(Err(error)) => {
                    if first_error.is_none() && !matches!(error, EngineError::Cancelled) {
                        first_error = Some(error);
                    }
                    cancel.cancel();
                }
                Ok(Ok(())) => {}
                Err(join_error) => {
                    if !join_error.is_cancelled() && first_error.is_none() {
                        first_error = Some(EngineError::Other(format!(
                            "transfer worker panicked: {join_error}"
                        )));
                    }
                    cancel.cancel();
                }
            }
        }

        ticker_cancel.cancel();
        let _ = ticker.await;

        if let Some(error) = first_error {
            self.checkpoint(&record.id, &board)?;
            return Err(error);
        }
        if cancel.is_cancelled() {
            self.checkpoint(&record.id, &board)?;
            return Err(EngineError::Cancelled);
        }
        if !board.is_complete() {
            self.checkpoint(&record.id, &board)?;
            return Err(EngineError::IncompleteTransfer);
        }

        self.checkpoint(&record.id, &board)?;
        self.finalize_file((*record).clone(), &part).await
    }

    async fn segment_worker(
        &self,
        record: &DownloadRecord,
        client: &Client,
        board: Arc<SegmentBoard>,
        mut index: u32,
        cancel: &CancellationToken,
        throttle: Arc<Throttle>,
    ) -> EngineResult<()> {
        loop {
            self.download_segment(record, client, &board, index, cancel, &throttle)
                .await?;
            match board.take_split(MIN_SEGMENT_BYTES) {
                Some(segment) => {
                    debug!(
                        download = %record.id,
                        index = segment.index,
                        "split an unfinished range"
                    );
                    index = segment.index;
                }
                None => return Ok(()),
            }
        }
    }

    async fn download_segment(
        &self,
        record: &DownloadRecord,
        client: &Client,
        board: &SegmentBoard,
        index: u32,
        cancel: &CancellationToken,
        throttle: &Throttle,
    ) -> EngineResult<()> {
        let mut attempt = 0_u32;
        loop {
            match self
                .try_segment(record, client, board, index, cancel, throttle)
                .await
            {
                Ok(()) => return Ok(()),
                Err(error)
                    if error.is_retryable()
                        && attempt + 1 < self.inner.config.retry.max_attempts =>
                {
                    attempt = attempt.saturating_add(1);
                    let delay = self.inner.config.retry.delay(attempt);
                    debug!(
                        download = %record.id,
                        index,
                        attempt,
                        delay_ms = delay.as_millis(),
                        %error,
                        "retrying byte range"
                    );
                    tokio::select! {
                        () = cancel.cancelled() => return Err(EngineError::Cancelled),
                        () = tokio::time::sleep(delay) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn try_segment(
        &self,
        record: &DownloadRecord,
        client: &Client,
        board: &SegmentBoard,
        index: u32,
        cancel: &CancellationToken,
        throttle: &Throttle,
    ) -> EngineResult<()> {
        let Some(segment) = board.segment(index) else {
            return Ok(());
        };
        if segment.state == SegmentState::Complete {
            return Ok(());
        }

        let offset = segment.next_offset();
        let current_end = board.end_for(index).unwrap_or(segment.end_byte);
        if offset > current_end {
            board.mark_complete(index);
            return Ok(());
        }

        let part = self.inner.config.part_path(&record.save_dir, record.id);
        let mut request = client
            .get(record.url.clone())
            .header(RANGE, format!("bytes={offset}-{current_end}"));
        if let Some(validator) = record.etag.as_ref().or(record.last_modified.as_ref()) {
            request = request.header(IF_RANGE, validator);
        }
        let request = http::apply_headers(request, record)?;
        let response = tokio::select! {
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
            response = request.send() => response?,
        };
        let status = response.status();

        match status {
            reqwest::StatusCode::PARTIAL_CONTENT => {
                if let Some((start, end, total)) = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(http::parse_content_range)
                {
                    if start != offset {
                        return Err(EngineError::ValidatorChanged);
                    }
                    if let Some(total) = total {
                        if total != board.total_bytes() {
                            return Err(EngineError::ValidatorChanged);
                        }
                    }
                    if end < current_end {
                        return Err(EngineError::IncompleteTransfer);
                    }
                } else {
                    return Err(EngineError::IncompleteTransfer);
                }
            }
            reqwest::StatusCode::OK => {
                let whole_file = offset == 0 && current_end + 1 == board.total_bytes();
                if !whole_file {
                    return Err(if record.etag.is_some() || record.last_modified.is_some() {
                        EngineError::ValidatorChanged
                    } else {
                        EngineError::RangesUnsupported
                    });
                }
            }
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
                board.mark_complete(index);
                return Ok(());
            }
            status if status.is_success() => {
                return Err(EngineError::HttpStatus {
                    status: status.as_u16(),
                    url: record.url.to_string(),
                });
            }
            status => {
                return Err(EngineError::HttpStatus {
                    status: status.as_u16(),
                    url: record.url.to_string(),
                });
            }
        }

        if let Some(encoding) = response
            .headers()
            .get(CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
        {
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(EngineError::Other(format!(
                    "server forced content encoding {encoding:?}; cannot verify byte ranges"
                )));
            }
        }

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&part)
            .await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut stream = response.bytes_stream();
        let mut position = offset;
        loop {
            let end_now = board.end_for(index).unwrap_or(current_end);
            if position > end_now {
                break;
            }
            let next = tokio::select! {
                () = cancel.cancelled() => return Err(EngineError::Cancelled),
                next = stream.next() => next,
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(EngineError::Network)?;
            let allowed = (end_now - position + 1).min(u64::try_from(chunk.len()).unwrap_or(0));
            if allowed == 0 {
                break;
            }
            let allowed_usize = usize::try_from(allowed).unwrap_or(chunk.len());
            if throttle.is_limited() {
                tokio::select! {
                    () = cancel.cancelled() => return Err(EngineError::Cancelled),
                    () = throttle.acquire(allowed) => {}
                }
            }
            file.write_all(&chunk[..allowed_usize]).await?;
            board.add_downloaded(index, allowed);
            position = position.saturating_add(allowed);
            if board.is_complete() {
                break;
            }
        }

        if position <= board.end_for(index).unwrap_or(current_end) {
            return Err(EngineError::IncompleteTransfer);
        }
        board.mark_complete(index);
        Ok(())
    }

    async fn run_single_stream(
        &self,
        mut record: DownloadRecord,
        client: &Client,
        cancel: &CancellationToken,
        throttle: Arc<Throttle>,
    ) -> EngineResult<()> {
        let part = self.inner.config.part_path(&record.save_dir, record.id);
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut request = http::apply_headers(client.get(record.url.clone()), &record)?;
        if let Some(validator) = record.etag.as_ref().or(record.last_modified.as_ref()) {
            request = request.header(IF_RANGE, validator);
        }

        let response = tokio::select! {
            () = cancel.cancelled() => return Err(EngineError::Cancelled),
            response = request.send() => response?,
        };
        let status = response.status();
        if !status.is_success() {
            return Err(EngineError::HttpStatus {
                status: status.as_u16(),
                url: record.url.to_string(),
            });
        }
        if let Some(encoding) = response
            .headers()
            .get(CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
        {
            if !encoding.eq_ignore_ascii_case("identity") {
                return Err(EngineError::Other(format!(
                    "server forced content encoding {encoding:?}"
                )));
            }
        }

        if let Some(length) = response.content_length() {
            record.total_bytes = Some(length);
        }
        record.resumable = false;
        record.state = DownloadState::Downloading;
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);

        let mut file = tokio::fs::File::create(&part).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        let mut last_report = Instant::now();
        let mut last_bytes = 0_u64;

        loop {
            let next = tokio::select! {
                () = cancel.cancelled() => return Err(EngineError::Cancelled),
                next = stream.next() => next,
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(EngineError::Network)?;
            if throttle.is_limited() {
                tokio::select! {
                    () = cancel.cancelled() => return Err(EngineError::Cancelled),
                    () = throttle.acquire(u64::try_from(chunk.len()).unwrap_or(0)) => {}
                }
            }
            file.write_all(&chunk).await?;
            downloaded = downloaded.saturating_add(u64::try_from(chunk.len()).unwrap_or(0));

            if last_report.elapsed() >= PROGRESS_INTERVAL {
                self.report_single(&mut record, downloaded, last_bytes, last_report);
                last_report = Instant::now();
                last_bytes = downloaded;
            }
        }

        file.flush().await?;
        drop(file);

        if let Some(total) = record.total_bytes {
            if downloaded != total {
                return Err(EngineError::IncompleteTransfer);
            }
        }
        record.downloaded_bytes = downloaded;
        self.inner.store.save_download(&record)?;
        self.finalize_file(record, &part).await
    }

    fn report_single(
        &self,
        record: &mut DownloadRecord,
        downloaded: u64,
        last_bytes: u64,
        last_report: Instant,
    ) {
        let elapsed = last_report.elapsed().as_secs_f64().max(0.001);
        let speed = ((downloaded.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
        record.downloaded_bytes = downloaded;
        record.updated_at = Utc::now();
        if let Err(error) = self.inner.store.save_download(record) {
            warn!(%error, "failed to checkpoint single-stream progress");
        }
        let mut progress = DownloadProgress::from_record(record, speed);
        progress.connections = vec![SegmentProgress {
            index: 0,
            start_byte: 0,
            end_byte: record.total_bytes.unwrap_or(downloaded).saturating_sub(1),
            downloaded,
            speed_bps: speed,
            active: true,
        }];
        self.emit(EngineEvent::Progress { progress });
    }

    fn spawn_ticker(
        &self,
        id: DownloadId,
        board: Arc<SegmentBoard>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let engine = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(CHECKPOINT_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_report = Instant::now();
            let mut last_bytes = board.downloaded_bytes();
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    _ = interval.tick() => {
                        let downloaded = board.downloaded_bytes();
                        let elapsed = last_report.elapsed().as_secs_f64().max(0.001);
                        let speed = ((downloaded.saturating_sub(last_bytes)) as f64 / elapsed) as u64;
                        last_report = Instant::now();
                        last_bytes = downloaded;

                        let Ok(Some(mut record)) = engine.inner.store.load_download(id) else {
                            continue;
                        };
                        record.downloaded_bytes = downloaded;
                        record.updated_at = Utc::now();
                        if let Err(error) = engine.inner.store.save_download(&record) {
                            warn!(%error, "failed to checkpoint progress");
                        }
                        let segments = board.snapshot();
                        if let Err(error) = engine.inner.store.replace_segments(id, &segments) {
                            warn!(%error, "failed to checkpoint segments");
                        }

                        let mut progress = DownloadProgress::from_record(&record, speed);
                        progress.connections = segments
                            .iter()
                            .map(|segment| SegmentProgress {
                                index: segment.index,
                                start_byte: segment.start_byte,
                                end_byte: segment.end_byte,
                                downloaded: segment.downloaded,
                                speed_bps: 0,
                                active: segment.state == SegmentState::Active
                                    && !segment.is_complete(),
                            })
                            .collect();
                        engine.emit(EngineEvent::Progress { progress });
                    }
                }
            }
        })
    }

    fn checkpoint(&self, id: &DownloadId, board: &SegmentBoard) -> EngineResult<()> {
        let Some(mut record) = self.inner.store.load_download(*id)? else {
            return Ok(());
        };
        let segments = board.snapshot();
        record.downloaded_bytes = segments.iter().map(|segment| segment.downloaded).sum();
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.inner.store.replace_segments(*id, &segments)?;
        Ok(())
    }

    async fn finalize_file(&self, mut record: DownloadRecord, part: &Path) -> EngineResult<()> {
        record.state = DownloadState::Verifying;
        record.updated_at = Utc::now();
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);

        if let Some(checksum) = record.checksum.clone() {
            match checksum::verify_file(part, &checksum).await {
                Ok(actual) => {
                    record.checksum_actual = Some(actual);
                }
                Err(EngineError::ChecksumMismatch { expected, actual }) => {
                    record.checksum_actual = Some(actual.clone());
                    self.inner.store.save_download(&record)?;
                    return Err(EngineError::ChecksumMismatch { expected, actual });
                }
                Err(error) => return Err(error),
            }
        }

        let destination = unique_path(&record.save_dir, &record.filename);
        let file = tokio::fs::OpenOptions::new().write(true).open(part).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(part, &destination).await?;
        sync_parent(&record.save_dir);

        if let Some(name) = destination.file_name().and_then(|name| name.to_str()) {
            record.filename = sanitize_filename(name);
        }
        record.state = DownloadState::Completed;
        record.completed_at = Some(Utc::now());
        record.updated_at = Utc::now();
        record.error = None;
        if let Some(total) = record.total_bytes {
            record.downloaded_bytes = total;
        }
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);
        self.emit(EngineEvent::Completed {
            id: record.id,
            path: destination,
        });
        cleanup_part_dir(&self.inner.config.part_path(&record.save_dir, record.id)).await;
        Ok(())
    }

    async fn finalize_empty(&self, mut record: DownloadRecord) -> EngineResult<()> {
        tokio::fs::create_dir_all(&record.save_dir).await?;
        let destination = unique_path(&record.save_dir, &record.filename);
        tokio::fs::write(&destination, []).await?;
        if let Some(name) = destination.file_name().and_then(|name| name.to_str()) {
            record.filename = sanitize_filename(name);
        }
        record.state = DownloadState::Completed;
        record.total_bytes = Some(0);
        record.downloaded_bytes = 0;
        record.completed_at = Some(Utc::now());
        record.updated_at = Utc::now();
        record.error = None;
        self.inner.store.save_download(&record)?;
        self.emit_state(&record);
        self.emit(EngineEvent::Completed {
            id: record.id,
            path: destination,
        });
        Ok(())
    }

    async fn cleanup_part(&self, record: &DownloadRecord) {
        let part = self.inner.config.part_path(&record.save_dir, record.id);
        let _ = tokio::fs::remove_file(&part).await;
        cleanup_part_dir(&part).await;
    }
}

async fn cleanup_part_dir(part: &Path) {
    if let Some(parent) = part.parent() {
        let _ = tokio::fs::remove_dir(parent).await;
    }
}

fn sync_parent(directory: &Path) {
    #[cfg(unix)]
    {
        if let Ok(file) = std::fs::File::open(directory) {
            let _ = file.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
    }
}

/// Convenience path type for callers building a spec.
#[must_use]
pub fn destination_for(record: &DownloadRecord) -> PathBuf {
    record.destination()
}

/// Re-exported for tests that need the part naming rule.
#[must_use]
pub fn part_path(config: &EngineConfig, record: &DownloadRecord) -> PathBuf {
    config.part_path(&record.save_dir, record.id)
}
