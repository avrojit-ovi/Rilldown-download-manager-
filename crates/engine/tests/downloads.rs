use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, RANGE,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use rilldown_engine::{
    Checksum, ChecksumAlgorithm, DEFAULT_QUEUE_ID, DownloadId, DownloadRecord, DownloadSpec,
    DownloadState, Engine, EngineConfig, EngineEvent, Queue, RetryConfig, Url,
};
use sha2::Digest;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout};

#[derive(Clone)]
struct TestState {
    payload: Arc<Vec<u8>>,
    range_requests: Arc<AtomicUsize>,
    failures_left: Arc<AtomicUsize>,
}

async fn start_server(payload: Vec<u8>, failures: usize) -> (String, TestState) {
    let state = TestState {
        payload: Arc::new(payload),
        range_requests: Arc::new(AtomicUsize::new(0)),
        failures_left: Arc::new(AtomicUsize::new(failures)),
    };
    let app = Router::new()
        .route("/file.bin", get(file))
        .route("/norange.bin", get(no_range))
        .route("/flaky.bin", get(flaky))
        .route("/slow.bin", get(slow))
        .route("/unknown.bin", get(unknown_length))
        .route("/disposition.bin", get(disposition))
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), state)
}

async fn file(State(state): State<TestState>, method: Method, headers: HeaderMap) -> Response {
    ranged_response(&state, &method, &headers, false, false)
}

async fn slow(State(state): State<TestState>, method: Method, headers: HeaderMap) -> Response {
    ranged_response(&state, &method, &headers, true, false)
}

async fn flaky(State(state): State<TestState>, method: Method, headers: HeaderMap) -> Response {
    ranged_response(&state, &method, &headers, false, true)
}

async fn no_range(State(state): State<TestState>, method: Method) -> Response {
    if method == Method::HEAD {
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, state.payload.len())
            .body(Body::empty())
            .unwrap();
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_LENGTH, state.payload.len())
        .body(Body::from(state.payload.as_ref().clone()))
        .unwrap()
}

async fn unknown_length(State(state): State<TestState>, method: Method) -> Response {
    if method == Method::HEAD {
        return Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap();
    }
    let stream = futures_util::stream::iter(
        state
            .payload
            .chunks(32 * 1024)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>(),
    );
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn disposition(State(state): State<TestState>, method: Method) -> Response {
    let builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_LENGTH, state.payload.len())
        .header(
            CONTENT_DISPOSITION,
            "attachment; filename*=UTF-8''na%C3%AFve%20file.txt",
        );
    if method == Method::HEAD {
        builder.body(Body::empty()).unwrap()
    } else {
        builder
            .body(Body::from(state.payload.as_ref().clone()))
            .unwrap()
    }
}

fn ranged_response(
    state: &TestState,
    method: &Method,
    headers: &HeaderMap,
    slow: bool,
    flaky: bool,
) -> Response {
    let total = state.payload.len() as u64;
    let etag = "\"v1\"";

    if *method == Method::HEAD {
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, total)
            .header(ACCEPT_RANGES, "bytes")
            .header(ETAG, etag)
            .body(Body::empty())
            .unwrap();
    }

    if flaky {
        let remaining = state.failures_left.load(Ordering::SeqCst);
        if remaining > 0 {
            state.failures_left.store(remaining - 1, Ordering::SeqCst);
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    state.range_requests.fetch_add(1, Ordering::SeqCst);
    if let Some(if_range) = headers.get(IF_RANGE).and_then(|value| value.to_str().ok()) {
        if if_range != etag {
            return Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_LENGTH, total)
                .header(ETAG, etag)
                .body(Body::from(state.payload.as_ref().clone()))
                .unwrap();
        }
    }

    let Some((start, end)) = parse_range(headers, total) else {
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, total)
            .header(ACCEPT_RANGES, "bytes")
            .header(ETAG, etag)
            .body(Body::from(state.payload.as_ref().clone()))
            .unwrap();
    };

    let length = end - start + 1;
    let body = if slow {
        slow_body(state.payload.as_ref().clone(), start as usize, end as usize)
    } else {
        Body::from(state.payload[start as usize..=end as usize].to_vec())
    };

    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
        .header(CONTENT_LENGTH, length)
        .header(ACCEPT_RANGES, "bytes")
        .header(ETAG, etag)
        .body(body)
        .unwrap()
}

fn slow_body(payload: Vec<u8>, start: usize, end: usize) -> Body {
    let stream = futures_util::stream::unfold(
        (payload, start, end),
        |(payload, position, end)| async move {
            if position > end {
                return None;
            }
            sleep(Duration::from_millis(2)).await;
            let chunk_end = (position + 16 * 1024 - 1).min(end);
            let chunk = Bytes::copy_from_slice(&payload[position..=chunk_end]);
            Some((
                Ok::<_, std::io::Error>(chunk),
                (payload, chunk_end + 1, end),
            ))
        },
    );
    Body::from_stream(stream)
}

fn parse_range(headers: &HeaderMap, total: u64) -> Option<(u64, u64)> {
    let value = headers.get(RANGE)?.to_str().ok()?;
    let spec = value.strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    Some((start, end))
}

fn test_payload(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index * 31 + 7) as u8).collect()
}

fn setup() -> (tempfile::TempDir, EngineConfig, Engine) {
    let directory = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::new(
        directory.path().join("data"),
        directory.path().join("downloads"),
    );
    config.retry = RetryConfig {
        max_attempts: 5,
        base_delay_ms: 5,
        max_delay_ms: 20,
    };
    let engine = Engine::new(config.clone()).unwrap();
    (directory, config, engine)
}

async fn wait_terminal(engine: &Engine, id: DownloadId) -> DownloadRecord {
    timeout(Duration::from_secs(20), async {
        loop {
            let record = engine.get(id).unwrap();
            if record.state.is_terminal() {
                return record;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("download did not reach a terminal state")
}

async fn add(engine: &Engine, url: String, connections: u8) -> DownloadRecord {
    let mut spec = DownloadSpec::new(
        Url::parse(&url).unwrap(),
        engine.config().download_dir.clone(),
    );
    spec.connections = connections;
    engine.add(spec).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_download_writes_exact_bytes() {
    let payload = test_payload(2 * 1024 * 1024 + 123);
    let (base, state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();
    let mut events = engine.subscribe();

    let record = add(&engine, format!("{base}/file.bin"), 4).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(
        std::fs::read(completed.destination()).unwrap(),
        payload,
        "downloaded file differs"
    );
    assert!(
        state.range_requests.load(Ordering::SeqCst) >= 2,
        "expected multiple range requests"
    );

    let mut saw_completed = false;
    while let Ok(event) = events.try_recv() {
        if matches!(event, EngineEvent::Completed { .. }) {
            saw_completed = true;
        }
    }
    assert!(saw_completed, "completion event was not emitted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_without_ranges_falls_back_to_one_stream() {
    let payload = test_payload(512 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();

    let record = add(&engine, format!("{base}/norange.bin"), 8).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert!(!completed.resumable);
    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_transient_server_errors() {
    let payload = test_payload(1024 * 1024);
    let (base, state) = start_server(payload.clone(), 2).await;
    let (_directory, _config, engine) = setup();

    let record = add(&engine, format!("{base}/flaky.bin"), 2).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
    assert!(state.range_requests.load(Ordering::SeqCst) >= 2);
    assert_eq!(state.failures_left.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uses_content_disposition_filename() {
    let payload = test_payload(64 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();

    let record = add(&engine, format!("{base}/disposition.bin"), 2).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(completed.filename, "naïve file.txt");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verifies_checksums_and_reports_mismatch() {
    let payload = test_payload(256 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();

    let digest = hex::encode(sha2::Sha256::digest(&payload));
    let mut spec = DownloadSpec::new(
        Url::parse(&format!("{base}/file.bin")).unwrap(),
        engine.config().download_dir.clone(),
    );
    spec.connections = 2;
    spec.checksum = Some(Checksum {
        algorithm: ChecksumAlgorithm::Sha256,
        expected_hex: digest.clone(),
    });
    let record = engine.add(spec).await.unwrap();
    let completed = wait_terminal(&engine, record.id).await;
    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(completed.checksum_actual.as_deref(), Some(digest.as_str()));

    let mut bad = DownloadSpec::new(
        Url::parse(&format!("{base}/file.bin")).unwrap(),
        engine.config().download_dir.clone(),
    );
    bad.filename = Some("bad-checksum.bin".to_string());
    bad.connections = 1;
    bad.checksum = Some(Checksum {
        algorithm: ChecksumAlgorithm::Sha256,
        expected_hex: "00".repeat(32),
    });
    let bad_record = engine.add(bad).await.unwrap();
    let failed = wait_terminal(&engine, bad_record.id).await;
    assert_eq!(failed.state, DownloadState::Failed, "{failed:?}");
    assert!(
        !engine
            .config()
            .download_dir
            .join("bad-checksum.bin")
            .exists(),
        "a checksum mismatch must not publish the final file"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_checkpoints_and_resume_finishes_exactly() {
    let payload = test_payload(8 * 1024 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, config, engine) = setup();

    let record = add(&engine, format!("{base}/slow.bin"), 4).await;
    sleep(Duration::from_millis(180)).await;
    engine.pause(record.id).await.unwrap();

    let paused = engine.get(record.id).unwrap();
    assert_eq!(paused.state, DownloadState::Paused, "{paused:?}");
    assert!(
        paused.downloaded_bytes > 0,
        "pause should checkpoint received bytes"
    );
    assert!(
        paused.downloaded_bytes < payload.len() as u64,
        "the pause happened too late to test resume"
    );
    assert!(
        config.part_path(&paused.save_dir, paused.id).exists(),
        "partial data must survive a pause"
    );

    engine.resume(record.id).await.unwrap();
    let completed = wait_terminal(&engine, record.id).await;
    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_unknown_length_without_ranges() {
    let payload = test_payload(384 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();

    let record = add(&engine, format!("{base}/unknown.bin"), 4).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(completed.total_bytes, None);
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
}

#[tokio::test]
async fn built_in_queue_exists_and_limits_round_trip() {
    let (_directory, _config, engine) = setup();
    let queues = engine.queues().unwrap();
    assert_eq!(queues[0].id, DEFAULT_QUEUE_ID);

    let created = engine
        .save_queue(Queue {
            id: "night".to_string(),
            name: "Night".to_string(),
            max_concurrent: 2,
            speed_limit_bps: None,
            created_at: chrono::Utc::now(),
        })
        .unwrap();
    assert_eq!(created.max_concurrent, 2);

    let queues = engine.queues().unwrap();
    assert!(queues.iter().any(|queue| queue.id == "night"));

    assert!(engine.remove_queue(DEFAULT_QUEUE_ID).is_err());
    assert!(engine.remove_queue("night").unwrap());
    assert!(!engine.remove_queue("night").unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_limit_admits_one_transfer_at_a_time() {
    let payload = test_payload(4 * 1024 * 1024);
    let (base, _state) = start_server(payload, 0).await;
    let (_directory, _config, engine) = setup();
    engine
        .save_queue(Queue {
            id: "single".to_string(),
            name: "Single".to_string(),
            max_concurrent: 1,
            speed_limit_bps: None,
            created_at: chrono::Utc::now(),
        })
        .unwrap();

    let mut ids = Vec::new();
    for _ in 0..2 {
        let mut spec = DownloadSpec::new(
            Url::parse(&format!("{base}/slow.bin")).unwrap(),
            engine.config().download_dir.clone(),
        );
        spec.connections = 2;
        spec.queue_id = Some("single".to_string());
        ids.push(engine.add(spec).await.unwrap().id);
    }

    let mut active_id = None;
    timeout(Duration::from_secs(5), async {
        loop {
            let records: Vec<_> = ids.iter().map(|id| engine.get(*id).unwrap()).collect();
            let active = records
                .iter()
                .filter(|record| record.state.is_active())
                .count();
            let queued = records
                .iter()
                .filter(|record| record.state == DownloadState::Queued)
                .count();
            if active == 1 && queued == 1 {
                active_id = records
                    .iter()
                    .find(|record| record.state.is_active())
                    .map(|record| record.id);
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the queue should admit exactly one transfer");

    let running = active_id.unwrap();
    engine.pause(running).await.unwrap();
    let other = *ids.iter().find(|id| **id != running).unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            if engine.get(other).unwrap().state.is_active() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("pausing the first transfer should admit the second");

    for id in ids {
        engine.cancel(id).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_download_speed_limit_slows_the_transfer() {
    let payload = test_payload(200 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();

    let mut spec = DownloadSpec::new(
        Url::parse(&format!("{base}/file.bin")).unwrap(),
        engine.config().download_dir.clone(),
    );
    spec.connections = 2;
    spec.speed_limit_bps = Some(100 * 1024);
    let started = std::time::Instant::now();
    let record = engine.add(spec).await.unwrap();
    let completed = wait_terminal(&engine, record.id).await;
    let elapsed = started.elapsed();

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
    assert!(
        elapsed >= Duration::from_millis(1200),
        "200 KiB at 100 KiB/s finished too quickly: {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_speed_limit_slows_the_transfer() {
    let payload = test_payload(200 * 1024);
    let (base, _state) = start_server(payload.clone(), 0).await;
    let (_directory, _config, engine) = setup();
    engine.set_global_speed_limit(Some(100 * 1024));
    assert_eq!(engine.global_speed_limit(), Some(100 * 1024));

    let started = std::time::Instant::now();
    let record = add(&engine, format!("{base}/file.bin"), 2).await;
    let completed = wait_terminal(&engine, record.id).await;
    let elapsed = started.elapsed();

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(std::fs::read(completed.destination()).unwrap(), payload);
    assert!(
        elapsed >= Duration::from_millis(1200),
        "the global limit was not applied: {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_assigns_a_category() {
    let payload = test_payload(64 * 1024);
    let (base, _state) = start_server(payload, 0).await;
    let (_directory, _config, engine) = setup();

    let record = add(&engine, format!("{base}/disposition.bin"), 2).await;
    let completed = wait_terminal(&engine, record.id).await;

    assert_eq!(completed.state, DownloadState::Completed, "{completed:?}");
    assert_eq!(completed.category.as_deref(), Some("documents"));
    assert_eq!(completed.filename, "na\u{ef}ve file.txt");
}
