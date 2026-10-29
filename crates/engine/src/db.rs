//! SQLite persistence.
//!
//! One connection is owned by the store and protected by a mutex. The engine's
//! async workers never hold the lock across an await point; checkpoints are
//! short, synchronous transactions. WAL mode keeps UI reads independent of
//! writer commits.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use url::Url;
use uuid::Uuid;

use crate::error::{EngineError, EngineResult};
use crate::types::{
    Checksum, DEFAULT_QUEUE_ID, DownloadFilter, DownloadId, DownloadRecord, DownloadState, Queue,
    Segment, SegmentState,
};

const MIGRATIONS: &[&str] = &[include_str!("../migrations/001_init.sql")];

const SELECT_DOWNLOAD: &str = "
    SELECT id, url, final_url, save_dir, filename, custom_filename, state, total_bytes,
           downloaded_bytes, resumable, etag, last_modified, content_type, error,
           checksum_algorithm, checksum_expected, checksum_actual, created_at, updated_at,
           completed_at, queue_id, category, connections, speed_limit_bps, headers_json,
           cookies, referrer, user_agent, proxy, mirrors_json
    FROM downloads
";

const UPSERT_DOWNLOAD: &str = "
    INSERT INTO downloads (
        id, url, final_url, save_dir, filename, custom_filename, state, total_bytes,
        downloaded_bytes, resumable, etag, last_modified, content_type, error,
        checksum_algorithm, checksum_expected, checksum_actual, created_at, updated_at,
        completed_at, queue_id, category, connections, speed_limit_bps, headers_json,
        cookies, referrer, user_agent, proxy, mirrors_json
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
        ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30
    )
    ON CONFLICT(id) DO UPDATE SET
        url = excluded.url,
        final_url = excluded.final_url,
        save_dir = excluded.save_dir,
        filename = excluded.filename,
        custom_filename = excluded.custom_filename,
        state = excluded.state,
        total_bytes = excluded.total_bytes,
        downloaded_bytes = excluded.downloaded_bytes,
        resumable = excluded.resumable,
        etag = excluded.etag,
        last_modified = excluded.last_modified,
        content_type = excluded.content_type,
        error = excluded.error,
        checksum_algorithm = excluded.checksum_algorithm,
        checksum_expected = excluded.checksum_expected,
        checksum_actual = excluded.checksum_actual,
        updated_at = excluded.updated_at,
        completed_at = excluded.completed_at,
        queue_id = excluded.queue_id,
        category = excluded.category,
        connections = excluded.connections,
        speed_limit_bps = excluded.speed_limit_bps,
        headers_json = excluded.headers_json,
        cookies = excluded.cookies,
        referrer = excluded.referrer,
        user_agent = excluded.user_agent,
        proxy = excluded.proxy,
        mirrors_json = excluded.mirrors_json
";

/// Persistent engine state.
pub struct Store {
    connection: Mutex<Connection>,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Open (or create) a database at `path`.
    pub fn open(path: impl AsRef<Path>) -> EngineResult<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Open a transient database, primarily for tests.
    pub fn open_in_memory() -> EngineResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> EngineResult<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        // In-memory databases report "memory" instead of "wal"; both are fine.
        let _mode: String =
            connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn migrate(&self) -> EngineResult<()> {
        let mut connection = self.lock();
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let applied = usize::try_from(version).unwrap_or(0);
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(migration)?;
            let next = i64::try_from(index + 1).unwrap_or(i64::MAX);
            transaction.pragma_update(None, "user_version", next)?;
            transaction.commit()?;
        }
        Ok(())
    }

    /// Reset in-flight states after an unclean shutdown.
    pub fn recover_interrupted(&self) -> EngineResult<usize> {
        let connection = self.lock();
        let now = Utc::now().timestamp_millis();
        let affected = connection.execute(
            "UPDATE downloads
             SET state = 'paused',
                 error = COALESCE(error, 'Interrupted before completion'),
                 updated_at = ?1
             WHERE state IN ('probing', 'downloading', 'merging', 'verifying')",
            [now],
        )?;
        Ok(affected)
    }

    /// Number of applied migrations.
    pub fn schema_version(&self) -> EngineResult<u32> {
        let connection = self.lock();
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        Ok(u32::try_from(version).unwrap_or(0))
    }

    /// Insert or replace a download record.
    pub fn save_download(&self, record: &DownloadRecord) -> EngineResult<()> {
        let connection = self.lock();
        let headers_json = serde_json::to_string(&record.headers)?;
        let mirrors_json = serde_json::to_string(&record.mirrors)?;
        let save_dir = record.save_dir.to_string_lossy().to_string();
        let resumable = i64::from(record.resumable);
        let (algorithm, expected) = match &record.checksum {
            Some(checksum) => (
                Some(checksum.algorithm.as_str()),
                Some(checksum.expected_hex.as_str()),
            ),
            None => (None, None),
        };

        connection.execute(
            UPSERT_DOWNLOAD,
            params![
                record.id.to_string(),
                record.url.as_str(),
                record.final_url.as_ref().map(Url::as_str),
                save_dir,
                record.filename,
                i64::from(record.custom_filename),
                record.state.as_str(),
                optional_u64_to_i64(record.total_bytes)?,
                u64_to_i64(record.downloaded_bytes)?,
                resumable,
                record.etag,
                record.last_modified,
                record.content_type,
                record.error,
                algorithm,
                expected,
                record.checksum_actual,
                record.created_at.timestamp_millis(),
                record.updated_at.timestamp_millis(),
                record.completed_at.map(|value| value.timestamp_millis()),
                record.queue_id,
                record.category,
                i64::from(record.connections),
                optional_u64_to_i64(record.speed_limit_bps)?,
                headers_json,
                record.cookies,
                record.referrer,
                record.user_agent,
                record.proxy,
                mirrors_json,
            ],
        )?;
        Ok(())
    }

    /// Load a single download, if it exists.
    pub fn load_download(&self, id: DownloadId) -> EngineResult<Option<DownloadRecord>> {
        let connection = self.lock();
        let raw = connection
            .query_row(
                &format!("{SELECT_DOWNLOAD} WHERE id = ?1"),
                [id.to_string()],
                raw_download,
            )
            .optional()?;
        raw.map(RawDownload::into_record).transpose()
    }

    /// List every download, newest first.
    pub fn list_downloads(&self) -> EngineResult<Vec<DownloadRecord>> {
        self.list_filtered(&DownloadFilter::default())
    }

    /// List downloads matching an in-memory filter.
    ///
    /// Filtering is done after loading because the table is small (thousands of
    /// rows at most) and it keeps the query surface simple. If that changes,
    /// push the predicates into SQL.
    pub fn list_filtered(&self, filter: &DownloadFilter) -> EngineResult<Vec<DownloadRecord>> {
        let connection = self.lock();
        let mut statement = connection.prepare(&format!(
            "{SELECT_DOWNLOAD} ORDER BY created_at DESC, id ASC"
        ))?;
        let rows = statement.query_map([], raw_download)?;
        let mut records = Vec::new();
        for row in rows {
            let record = row?.into_record()?;
            if filter.matches(&record) {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// Delete a download and its segments. Returns whether a row was removed.
    pub fn delete_download(&self, id: DownloadId) -> EngineResult<bool> {
        let connection = self.lock();
        let affected =
            connection.execute("DELETE FROM downloads WHERE id = ?1", [id.to_string()])?;
        Ok(affected > 0)
    }

    /// Replace all segment rows for a download in one transaction.
    pub fn replace_segments(&self, id: DownloadId, segments: &[Segment]) -> EngineResult<()> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM segments WHERE download_id = ?1",
            [id.to_string()],
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO segments (download_id, idx, start_byte, end_byte, downloaded, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for segment in segments {
                statement.execute(params![
                    id.to_string(),
                    i64::from(segment.index),
                    u64_to_i64(segment.start_byte)?,
                    u64_to_i64(segment.end_byte)?,
                    u64_to_i64(segment.downloaded)?,
                    segment.state.as_str(),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Load segment rows for a download, ordered by index.
    pub fn load_segments(&self, id: DownloadId) -> EngineResult<Vec<Segment>> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT idx, start_byte, end_byte, downloaded, state
             FROM segments WHERE download_id = ?1 ORDER BY idx ASC",
        )?;
        let rows = statement.query_map([id.to_string()], |row| {
            Ok(RawSegment {
                index: row.get(0)?,
                start_byte: row.get(1)?,
                end_byte: row.get(2)?,
                downloaded: row.get(3)?,
                state: row.get(4)?,
            })
        })?;

        let mut segments = Vec::new();
        for row in rows {
            segments.push(row?.into_segment()?);
        }
        Ok(segments)
    }

    /// Read a setting value.
    pub fn get_setting(&self, key: &str) -> EngineResult<Option<String>> {
        let connection = self.lock();
        let value = connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value)
    }

    /// Write a setting value.
    pub fn set_setting(&self, key: &str, value: &str) -> EngineResult<()> {
        let connection = self.lock();
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Remove a setting.
    pub fn delete_setting(&self, key: &str) -> EngineResult<()> {
        let connection = self.lock();
        connection.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Create the built-in queue if the database predates it.
    pub fn ensure_default_queue(&self) -> EngineResult<()> {
        let main = Queue::main();
        let connection = self.lock();
        connection.execute(
            "INSERT OR IGNORE INTO queues (id, name, max_concurrent, speed_limit_bps, created_at)
             VALUES (?1, ?2, 0, NULL, ?3)",
            params![main.id, main.name, main.created_at.timestamp_millis()],
        )?;
        Ok(())
    }

    /// List queues in creation order, with the built-in queue first.
    pub fn list_queues(&self) -> EngineResult<Vec<Queue>> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT id, name, max_concurrent, speed_limit_bps, created_at
             FROM queues ORDER BY (id = 'main') DESC, created_at ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let mut queues = Vec::new();
        for row in rows {
            let (id, name, max_concurrent, speed_limit, created_at) = row?;
            let max_concurrent = u32::try_from(max_concurrent).unwrap_or(0);
            queues.push(Queue {
                id,
                name,
                max_concurrent,
                speed_limit_bps: speed_limit
                    .map(|value| i64_to_u64(value, "queue speed limit"))
                    .transpose()?,
                created_at: millis_to_datetime(created_at)?,
            });
        }
        Ok(queues)
    }

    /// Insert or update a queue row.
    pub fn save_queue(&self, queue: &Queue) -> EngineResult<()> {
        let connection = self.lock();
        connection.execute(
            "INSERT INTO queues (id, name, max_concurrent, speed_limit_bps, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 max_concurrent = excluded.max_concurrent,
                 speed_limit_bps = excluded.speed_limit_bps",
            params![
                queue.id,
                queue.name,
                i64::from(queue.max_concurrent),
                optional_u64_to_i64(queue.speed_limit_bps)?,
                queue.created_at.timestamp_millis(),
            ],
        )?;
        Ok(())
    }

    /// Delete a queue and move its downloads back to the built-in queue.
    pub fn delete_queue(&self, id: &str) -> EngineResult<bool> {
        if id == DEFAULT_QUEUE_ID {
            return Err(EngineError::Other(
                "the built-in queue cannot be removed".into(),
            ));
        }
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE downloads SET queue_id = ?1 WHERE queue_id = ?2",
            [DEFAULT_QUEUE_ID, id],
        )?;
        let affected = transaction.execute("DELETE FROM queues WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(affected > 0)
    }
}

fn u64_to_i64(value: u64) -> EngineResult<i64> {
    i64::try_from(value)
        .map_err(|_| EngineError::Other("byte offset exceeds SQLite's integer range".into()))
}

fn optional_u64_to_i64(value: Option<u64>) -> EngineResult<Option<i64>> {
    value.map(u64_to_i64).transpose()
}

fn i64_to_u64(value: i64, field: &str) -> EngineResult<u64> {
    u64::try_from(value)
        .map_err(|_| EngineError::Other(format!("negative {field} in database: {value}")))
}

fn millis_to_datetime(value: i64) -> EngineResult<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value)
        .ok_or_else(|| EngineError::Other(format!("invalid timestamp in database: {value}")))
}

#[derive(Debug)]
struct RawDownload {
    id: String,
    url: String,
    final_url: Option<String>,
    save_dir: String,
    filename: String,
    custom_filename: i64,
    state: String,
    total_bytes: Option<i64>,
    downloaded_bytes: i64,
    resumable: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    content_type: Option<String>,
    error: Option<String>,
    checksum_algorithm: Option<String>,
    checksum_expected: Option<String>,
    checksum_actual: Option<String>,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
    queue_id: Option<String>,
    category: Option<String>,
    connections: i64,
    speed_limit_bps: Option<i64>,
    headers_json: String,
    cookies: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
    proxy: Option<String>,
    mirrors_json: String,
}

impl RawDownload {
    fn into_record(self) -> EngineResult<DownloadRecord> {
        let id = DownloadId(
            Uuid::parse_str(&self.id)
                .map_err(|error| EngineError::Other(format!("invalid download id: {error}")))?,
        );
        let url = Url::parse(&self.url)
            .map_err(|error| EngineError::InvalidUrl(format!("{}: {error}", self.url)))?;
        let final_url = self
            .final_url
            .as_deref()
            .map(Url::parse)
            .transpose()
            .map_err(|error| EngineError::InvalidUrl(error.to_string()))?;
        let state = self.state.parse::<DownloadState>()?;
        let checksum = match (self.checksum_algorithm, self.checksum_expected) {
            (Some(algorithm), Some(expected_hex)) => Some(Checksum {
                algorithm: algorithm.parse()?,
                expected_hex,
            }),
            (None, None) => None,
            _ => {
                return Err(EngineError::Other(
                    "incomplete checksum columns in database".into(),
                ));
            }
        };
        let connections = u8::try_from(self.connections).unwrap_or(1).clamp(1, 32);
        let headers = serde_json::from_str(&self.headers_json)?;
        let mirrors = serde_json::from_str(&self.mirrors_json)?;

        Ok(DownloadRecord {
            id,
            url,
            final_url,
            save_dir: PathBuf::from(self.save_dir),
            filename: self.filename,
            custom_filename: self.custom_filename != 0,
            state,
            total_bytes: self
                .total_bytes
                .map(|value| i64_to_u64(value, "total_bytes"))
                .transpose()?,
            downloaded_bytes: i64_to_u64(self.downloaded_bytes, "downloaded_bytes")?,
            resumable: self.resumable != 0,
            etag: self.etag,
            last_modified: self.last_modified,
            content_type: self.content_type,
            error: self.error,
            checksum,
            checksum_actual: self.checksum_actual,
            created_at: millis_to_datetime(self.created_at)?,
            updated_at: millis_to_datetime(self.updated_at)?,
            completed_at: self.completed_at.map(millis_to_datetime).transpose()?,
            queue_id: self.queue_id,
            category: self.category,
            connections,
            speed_limit_bps: self
                .speed_limit_bps
                .map(|value| i64_to_u64(value, "speed_limit_bps"))
                .transpose()?,
            headers,
            cookies: self.cookies,
            referrer: self.referrer,
            user_agent: self.user_agent,
            proxy: self.proxy,
            mirrors,
        })
    }
}

fn raw_download(row: &Row<'_>) -> rusqlite::Result<RawDownload> {
    Ok(RawDownload {
        id: row.get("id")?,
        url: row.get("url")?,
        final_url: row.get("final_url")?,
        save_dir: row.get("save_dir")?,
        filename: row.get("filename")?,
        custom_filename: row.get("custom_filename")?,
        state: row.get("state")?,
        total_bytes: row.get("total_bytes")?,
        downloaded_bytes: row.get("downloaded_bytes")?,
        resumable: row.get("resumable")?,
        etag: row.get("etag")?,
        last_modified: row.get("last_modified")?,
        content_type: row.get("content_type")?,
        error: row.get("error")?,
        checksum_algorithm: row.get("checksum_algorithm")?,
        checksum_expected: row.get("checksum_expected")?,
        checksum_actual: row.get("checksum_actual")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        completed_at: row.get("completed_at")?,
        queue_id: row.get("queue_id")?,
        category: row.get("category")?,
        connections: row.get("connections")?,
        speed_limit_bps: row.get("speed_limit_bps")?,
        headers_json: row.get("headers_json")?,
        cookies: row.get("cookies")?,
        referrer: row.get("referrer")?,
        user_agent: row.get("user_agent")?,
        proxy: row.get("proxy")?,
        mirrors_json: row.get("mirrors_json")?,
    })
}

#[derive(Debug)]
struct RawSegment {
    index: i64,
    start_byte: i64,
    end_byte: i64,
    downloaded: i64,
    state: String,
}

impl RawSegment {
    fn into_segment(self) -> EngineResult<Segment> {
        Ok(Segment {
            index: u32::try_from(self.index)
                .map_err(|_| EngineError::Other("invalid segment index".into()))?,
            start_byte: i64_to_u64(self.start_byte, "segment start")?,
            end_byte: i64_to_u64(self.end_byte, "segment end")?,
            downloaded: i64_to_u64(self.downloaded, "segment progress")?,
            state: self.state.parse::<SegmentState>()?,
        })
    }
}
