-- Rilldown schema v1.
-- Timestamps are UTC milliseconds. Byte offsets are stored as SQLite INTEGERs,
-- which are signed 64-bit; the engine rejects values that do not fit.

CREATE TABLE IF NOT EXISTS downloads (
    id                  TEXT PRIMARY KEY NOT NULL,
    url                 TEXT NOT NULL,
    final_url           TEXT,
    save_dir            TEXT NOT NULL,
    filename            TEXT NOT NULL,
    custom_filename     INTEGER NOT NULL DEFAULT 0,
    state               TEXT NOT NULL,
    total_bytes         INTEGER,
    downloaded_bytes    INTEGER NOT NULL DEFAULT 0,
    resumable           INTEGER NOT NULL DEFAULT 0,
    etag                TEXT,
    last_modified       TEXT,
    content_type        TEXT,
    error               TEXT,
    checksum_algorithm  TEXT,
    checksum_expected   TEXT,
    checksum_actual     TEXT,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    completed_at        INTEGER,
    queue_id            TEXT,
    category            TEXT,
    connections         INTEGER NOT NULL DEFAULT 8,
    speed_limit_bps     INTEGER,
    headers_json        TEXT NOT NULL DEFAULT '{}',
    cookies             TEXT,
    referrer            TEXT,
    user_agent          TEXT,
    proxy               TEXT,
    mirrors_json        TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS downloads_state_idx ON downloads (state);
CREATE INDEX IF NOT EXISTS downloads_created_idx ON downloads (created_at DESC);
CREATE INDEX IF NOT EXISTS downloads_queue_idx ON downloads (queue_id);

CREATE TABLE IF NOT EXISTS segments (
    download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_byte  INTEGER NOT NULL,
    end_byte    INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    state       TEXT NOT NULL DEFAULT 'pending',
    PRIMARY KEY (download_id, idx)
);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS queues (
    id              TEXT PRIMARY KEY NOT NULL,
    name            TEXT NOT NULL,
    max_concurrent  INTEGER NOT NULL DEFAULT 2,
    speed_limit_bps INTEGER,
    schedule_json   TEXT,
    action_json     TEXT,
    created_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS categories (
    id              TEXT PRIMARY KEY NOT NULL,
    name            TEXT NOT NULL,
    extensions_json TEXT NOT NULL DEFAULT '[]',
    save_dir        TEXT NOT NULL,
    is_default      INTEGER NOT NULL DEFAULT 0
);
