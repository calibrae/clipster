use clipster_common::error::ClipsterError;
use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

pub fn run(conn: &Connection) -> Result<(), ClipsterError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );",
    )
    .map_err(|e| ClipsterError::Database(e.to_string()))?;

    let current: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get::<_, u32>(0),
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;

    if current < 1 {
        apply_v1(conn)?;
        bump(conn, 1)?;
    }
    if current < 2 {
        apply_v2(conn)?;
        bump(conn, 2)?;
    }

    Ok(())
}

fn bump(conn: &Connection, version: u32) -> Result<(), ClipsterError> {
    conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [version])
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
    Ok(())
}

fn apply_v1(conn: &Connection) -> Result<(), ClipsterError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clips (
            id TEXT PRIMARY KEY,
            content_type TEXT NOT NULL,
            text_content TEXT,
            image_hash TEXT,
            image_mime TEXT,
            file_ref_path TEXT,
            content_hash TEXT NOT NULL,
            source_device TEXT NOT NULL,
            source_app TEXT,
            byte_size INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            is_favorite INTEGER NOT NULL DEFAULT 0,
            is_deleted INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_clips_created_at ON clips(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_clips_content_type ON clips(content_type);
        CREATE INDEX IF NOT EXISTS idx_clips_content_hash ON clips(content_hash);",
    )
    .map_err(|e| ClipsterError::Database(e.to_string()))?;
    Ok(())
}

fn apply_v2(conn: &Connection) -> Result<(), ClipsterError> {
    // Add state_modified_at column if it doesn't already exist (idempotent on fresh DBs after v1).
    let has_col: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('clips') WHERE name = 'state_modified_at'",
            [],
            |row| row.get::<_, i64>(0).map(|n| n > 0),
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;

    if !has_col {
        conn.execute(
            "ALTER TABLE clips ADD COLUMN state_modified_at TEXT",
            [],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE clips SET state_modified_at = created_at WHERE state_modified_at IS NULL",
            [],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_clips_state_modified_at ON clips(state_modified_at DESC);
        CREATE TABLE IF NOT EXISTS peers (
            device_id     TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            trust_status  TEXT NOT NULL,
            pinned_at     TEXT,
            last_seen     TEXT,
            last_addr     TEXT,
            last_sync_at  TEXT,
            capabilities  TEXT
        );",
    )
    .map_err(|e| ClipsterError::Database(e.to_string()))?;
    Ok(())
}
