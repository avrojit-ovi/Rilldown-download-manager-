use std::collections::BTreeMap;

use rilldown_engine::{
    Checksum, ChecksumAlgorithm, DownloadFilter, DownloadId, DownloadSpec, DownloadState, Segment,
    SegmentState, Store, Url,
};

fn spec(directory: &std::path::Path) -> DownloadSpec {
    let mut spec = DownloadSpec::new(
        Url::parse("https://example.com/files/report.pdf").unwrap(),
        directory.to_path_buf(),
    );
    spec.connections = 16;
    spec.headers
        .insert("X-Request".to_string(), "rilldown-test".to_string());
    spec.referrer = Some("https://example.com/".to_string());
    spec.checksum = Some(Checksum {
        algorithm: ChecksumAlgorithm::Sha256,
        expected_hex: "a".repeat(64),
    });
    spec
}

#[test]
fn download_round_trips() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("state.db")).unwrap();
    assert_eq!(store.schema_version().unwrap(), 1);

    let record = spec(directory.path()).into_record(DownloadId::new());
    store.save_download(&record).unwrap();
    let loaded = store.load_download(record.id).unwrap().unwrap();

    assert_eq!(loaded.id, record.id);
    assert_eq!(loaded.filename, "report.pdf");
    assert_eq!(loaded.connections, 16);
    assert_eq!(loaded.state, DownloadState::Queued);
    assert_eq!(loaded.headers["X-Request"], "rilldown-test");
    assert_eq!(loaded.referrer.as_deref(), Some("https://example.com/"));
    assert!(loaded.destination().ends_with("report.pdf"));
    assert_eq!(
        loaded.checksum.as_ref().unwrap().algorithm,
        ChecksumAlgorithm::Sha256
    );
}

#[test]
fn update_preserves_created_at() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut record = spec(directory.path()).into_record(DownloadId::new());
    store.save_download(&record).unwrap();

    let created_at = record.created_at;
    record.state = DownloadState::Completed;
    record.downloaded_bytes = 4096;
    record.total_bytes = Some(4096);
    record.completed_at = Some(record.updated_at);
    store.save_download(&record).unwrap();

    let loaded = store.load_download(record.id).unwrap().unwrap();
    assert_eq!(loaded.state, DownloadState::Completed);
    assert_eq!(loaded.downloaded_bytes, 4096);
    // Timestamps round-trip at millisecond precision.
    assert_eq!(
        loaded.created_at.timestamp_millis(),
        created_at.timestamp_millis()
    );
}

#[test]
fn segments_are_replaced_atomically_and_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let record = spec(directory.path()).into_record(DownloadId::new());
    store.save_download(&record).unwrap();

    let segments = vec![
        Segment {
            index: 0,
            start_byte: 0,
            end_byte: 99,
            downloaded: 100,
            state: SegmentState::Complete,
        },
        Segment {
            index: 1,
            start_byte: 100,
            end_byte: 199,
            downloaded: 40,
            state: SegmentState::Active,
        },
    ];
    store.replace_segments(record.id, &segments).unwrap();
    let loaded = store.load_segments(record.id).unwrap();
    assert_eq!(loaded, segments);

    let replacement = vec![Segment {
        index: 0,
        start_byte: 0,
        end_byte: 199,
        downloaded: 0,
        state: SegmentState::Pending,
    }];
    store.replace_segments(record.id, &replacement).unwrap();
    assert_eq!(store.load_segments(record.id).unwrap(), replacement);

    assert!(store.delete_download(record.id).unwrap());
    assert_eq!(store.load_segments(record.id).unwrap(), Vec::new());
    assert!(store.load_download(record.id).unwrap().is_none());
}

#[test]
fn settings_round_trip() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_setting("theme").unwrap(), None);
    store.set_setting("theme", "dark").unwrap();
    assert_eq!(store.get_setting("theme").unwrap().as_deref(), Some("dark"));
    store.set_setting("theme", "system").unwrap();
    assert_eq!(
        store.get_setting("theme").unwrap().as_deref(),
        Some("system")
    );
    store.delete_setting("theme").unwrap();
    assert_eq!(store.get_setting("theme").unwrap(), None);
}

#[test]
fn filtering_by_state_and_search() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();

    let mut first = spec(directory.path()).into_record(DownloadId::new());
    first.state = DownloadState::Completed;
    first.filename = "invoice.pdf".to_string();
    let mut second = spec(directory.path()).into_record(DownloadId::new());
    second.state = DownloadState::Downloading;
    second.filename = "holiday.mp4".to_string();
    store.save_download(&first).unwrap();
    store.save_download(&second).unwrap();

    let completed = DownloadFilter {
        states: vec![DownloadState::Completed],
        ..DownloadFilter::default()
    };
    let results = store.list_filtered(&completed).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].filename, "invoice.pdf");

    let search = DownloadFilter {
        search: Some("holiday".to_string()),
        ..DownloadFilter::default()
    };
    let results = store.list_filtered(&search).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].filename, "holiday.mp4");
}

#[test]
fn reopening_keeps_data_and_does_not_rerun_migrations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.db");
    let id = {
        let store = Store::open(&path).unwrap();
        let record = spec(directory.path()).into_record(DownloadId::new());
        store.save_download(&record).unwrap();
        record.id
    };

    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), 1);
    assert!(reopened.load_download(id).unwrap().is_some());
}

#[test]
fn headers_are_stored_as_valid_json() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let mut record = spec(directory.path()).into_record(DownloadId::new());
    let mut headers = BTreeMap::new();
    headers.insert("Accept".to_string(), "application/pdf".to_string());
    record.headers = headers;
    store.save_download(&record).unwrap();
    assert_eq!(
        store
            .load_download(record.id)
            .unwrap()
            .unwrap()
            .headers
            .get("Accept")
            .map(String::as_str),
        Some("application/pdf")
    );
}
