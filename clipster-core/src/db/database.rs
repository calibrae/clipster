use super::migrations;
use super::peers::{PeerRecord, TrustStatus};
use chrono::{DateTime, Utc};
use clipster_common::error::ClipsterError;
use clipster_common::models::{Clip, ClipListQuery};
use rusqlite::{Connection, OptionalExtension, params};
use std::str::FromStr;
use std::sync::Mutex;
use uuid::Uuid;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, ClipsterError> {
        let conn = Connection::open(path).map_err(|e| ClipsterError::Database(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn migrate(&self) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        migrations::run(&conn)
    }

    // ── Clip CRUD ─────────────────────────────────────────────────

    pub fn insert_clip(&self, clip: &Clip) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clips (id, content_type, text_content, image_hash, image_mime, file_ref_path, content_hash, source_device, source_app, byte_size, created_at, state_modified_at, is_favorite, is_deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                clip.id.to_string(),
                clip.content_type.to_string(),
                clip.text_content,
                clip.image_hash,
                clip.image_mime,
                clip.file_ref_path,
                clip.content_hash,
                clip.source_device,
                clip.source_app,
                clip.byte_size as i64,
                clip.created_at.to_rfc3339(),
                clip.state_modified_at.to_rfc3339(),
                clip.is_favorite as i32,
                clip.is_deleted as i32,
            ],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(())
    }

    /// LWW merge from a remote peer. INSERTs unknown clips, UPDATEs state
    /// when remote's state_modified_at is more recent.
    pub fn upsert_clip_from_peer(&self, remote: &Clip) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<DateTime<Utc>> = conn
            .query_row(
                "SELECT state_modified_at FROM clips WHERE id = ?1",
                params![remote.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .map(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .unwrap()
                    .with_timezone(&Utc)
            });

        match existing {
            None => {
                // Insert new clip preserving source_device (provenance).
                conn.execute(
                    "INSERT INTO clips (id, content_type, text_content, image_hash, image_mime, file_ref_path, content_hash, source_device, source_app, byte_size, created_at, state_modified_at, is_favorite, is_deleted)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        remote.id.to_string(),
                        remote.content_type.to_string(),
                        remote.text_content,
                        remote.image_hash,
                        remote.image_mime,
                        remote.file_ref_path,
                        remote.content_hash,
                        remote.source_device,
                        remote.source_app,
                        remote.byte_size as i64,
                        remote.created_at.to_rfc3339(),
                        remote.state_modified_at.to_rfc3339(),
                        remote.is_favorite as i32,
                        remote.is_deleted as i32,
                    ],
                )
                .map_err(|e| ClipsterError::Database(e.to_string()))?;
            }
            Some(local_ts) if remote.state_modified_at > local_ts => {
                conn.execute(
                    "UPDATE clips SET is_favorite = ?1, is_deleted = ?2, state_modified_at = ?3 WHERE id = ?4",
                    params![
                        remote.is_favorite as i32,
                        remote.is_deleted as i32,
                        remote.state_modified_at.to_rfc3339(),
                        remote.id.to_string(),
                    ],
                )
                .map_err(|e| ClipsterError::Database(e.to_string()))?;
            }
            Some(_) => { /* local wins, no-op */ }
        }
        Ok(())
    }

    pub fn get_clip(&self, id: &Uuid) -> Result<Clip, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM clips WHERE id = ?1 AND is_deleted = 0",
            params![id.to_string()],
            |row| Ok(row_to_clip(row)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                ClipsterError::NotFound(format!("clip {id}"))
            }
            other => ClipsterError::Database(other.to_string()),
        })
    }

    /// Like get_clip but does NOT filter is_deleted (used by sync merge).
    pub fn get_clip_any(&self, id: &Uuid) -> Result<Option<Clip>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM clips WHERE id = ?1",
            params![id.to_string()],
            |row| Ok(row_to_clip(row)),
        )
        .optional()
        .map_err(|e| ClipsterError::Database(e.to_string()))
    }

    pub fn has_recent_duplicate(
        &self,
        content_hash: &str,
        within_secs: i64,
    ) -> Result<bool, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(within_secs);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE content_hash = ?1 AND created_at > ?2 AND is_deleted = 0",
                params![content_hash, cutoff.to_rfc3339()],
                |row| row.get(0),
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    pub fn list_clips(&self, query: &ClipListQuery) -> Result<(Vec<Clip>, u64), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from("SELECT * FROM clips WHERE is_deleted = 0");
        let mut count_sql = String::from("SELECT COUNT(*) FROM clips WHERE is_deleted = 0");
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref ct) = query.content_type {
            conditions.push(format!("content_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(ct.clone()));
        }
        if let Some(ref search) = query.search {
            conditions.push(format!("text_content LIKE ?{}", param_values.len() + 1));
            param_values.push(Box::new(format!("%{search}%")));
        }
        if let Some(ref device) = query.device {
            conditions.push(format!("source_device = ?{}", param_values.len() + 1));
            param_values.push(Box::new(device.clone()));
        }
        if let Some(ref exclude) = query.exclude_device {
            conditions.push(format!("source_device != ?{}", param_values.len() + 1));
            param_values.push(Box::new(exclude.clone()));
        }
        if let Some(since) = query.since {
            conditions.push(format!("created_at > ?{}", param_values.len() + 1));
            param_values.push(Box::new(since.to_rfc3339()));
        }

        for cond in &conditions {
            sql.push_str(" AND ");
            sql.push_str(cond);
            count_sql.push_str(" AND ");
            count_sql.push_str(cond);
        }

        sql.push_str(" ORDER BY created_at DESC");

        let limit = query.limit.unwrap_or(50).min(200);
        let offset = query.offset.unwrap_or(0);
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let total: u64 = conn
            .query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))
            .map_err(|e| ClipsterError::Database(e.to_string()))?;

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        let clips = stmt
            .query_map(params_refs.as_slice(), |row| Ok(row_to_clip(row)))
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClipsterError::Database(e.to_string()))?;

        Ok((clips, total))
    }

    /// List clips changed since the given timestamp (created OR state-modified).
    /// Includes soft-deleted rows so tombstones propagate. Used by peer sync.
    pub fn list_clips_since(
        &self,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Clip>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let since_s = since.to_rfc3339();
        let limit = limit.min(1000);
        let mut stmt = conn
            .prepare(
                "SELECT * FROM clips
                 WHERE created_at > ?1 OR state_modified_at > ?1
                 ORDER BY state_modified_at ASC
                 LIMIT ?2",
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        let clips = stmt
            .query_map(params![since_s, limit as i64], |row| Ok(row_to_clip(row)))
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(clips)
    }

    pub fn soft_delete(&self, id: &Uuid) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let affected = conn
            .execute(
                "UPDATE clips SET is_deleted = 1, state_modified_at = ?2 WHERE id = ?1",
                params![id.to_string(), now],
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        if affected == 0 {
            return Err(ClipsterError::NotFound(format!("clip {id}")));
        }
        Ok(())
    }

    /// Soft-delete all non-favorite clips (or all clips if `keep_favorites` is false).
    pub fn delete_all(&self, keep_favorites: bool) -> Result<u64, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let sql = if keep_favorites {
            "UPDATE clips SET is_deleted = 1, state_modified_at = ?1 WHERE is_deleted = 0 AND is_favorite = 0"
        } else {
            "UPDATE clips SET is_deleted = 1, state_modified_at = ?1 WHERE is_deleted = 0"
        };
        let affected = conn
            .execute(sql, params![now])
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(affected as u64)
    }

    pub fn purge_older_than(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> Result<(u64, Vec<String>), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let cutoff = older_than.to_rfc3339();

        let mut stmt = conn
            .prepare(
                "SELECT image_hash FROM clips
                 WHERE is_favorite = 0
                   AND created_at < ?1
                   AND content_type = 'image'
                   AND image_hash IS NOT NULL",
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        let hashes: Vec<String> = stmt
            .query_map(params![cutoff], |row| row.get::<_, String>(0))
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        let affected = conn
            .execute(
                "DELETE FROM clips WHERE is_favorite = 0 AND created_at < ?1",
                params![cutoff],
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;

        let mut orphans = Vec::new();
        for h in hashes {
            let still_used: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM clips WHERE image_hash = ?1",
                    params![h],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if still_used == 0 {
                orphans.push(h);
            }
        }

        Ok((affected as u64, orphans))
    }

    pub fn toggle_favorite(&self, id: &Uuid) -> Result<bool, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE clips SET is_favorite = 1 - is_favorite, state_modified_at = ?2 WHERE id = ?1 AND is_deleted = 0",
            params![id.to_string(), now],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;

        let fav: bool = conn
            .query_row(
                "SELECT is_favorite FROM clips WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(fav)
    }

    // ── Peers ──────────────────────────────────────────────────────

    pub fn upsert_peer_discovery(
        &self,
        device_id: &str,
        name: &str,
        addr: &str,
        capabilities: Option<&str>,
    ) -> Result<TrustStatus, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        let existing: Option<String> = conn
            .query_row(
                "SELECT trust_status FROM peers WHERE device_id = ?1",
                params![device_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ClipsterError::Database(e.to_string()))?;

        match existing {
            Some(status_str) => {
                conn.execute(
                    "UPDATE peers SET name = ?2, last_seen = ?3, last_addr = ?4, capabilities = COALESCE(?5, capabilities) WHERE device_id = ?1",
                    params![device_id, name, now, addr, capabilities],
                )
                .map_err(|e| ClipsterError::Database(e.to_string()))?;
                TrustStatus::from_str(&status_str).map_err(ClipsterError::BadRequest)
            }
            None => {
                conn.execute(
                    "INSERT INTO peers (device_id, name, trust_status, last_seen, last_addr, capabilities) VALUES (?1, ?2, 'pending', ?3, ?4, ?5)",
                    params![device_id, name, now, addr, capabilities],
                )
                .map_err(|e| ClipsterError::Database(e.to_string()))?;
                Ok(TrustStatus::Pending)
            }
        }
    }

    pub fn set_peer_trust(
        &self,
        device_id: &str,
        status: TrustStatus,
    ) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        let pinned_at = if status == TrustStatus::Trusted {
            Some(now)
        } else {
            None
        };
        let affected = conn
            .execute(
                "UPDATE peers SET trust_status = ?1, pinned_at = COALESCE(?2, pinned_at) WHERE device_id = ?3",
                params![status.to_string(), pinned_at, device_id],
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        if affected == 0 {
            return Err(ClipsterError::NotFound(format!("peer {device_id}")));
        }
        Ok(())
    }

    pub fn list_peers(&self) -> Result<Vec<PeerRecord>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT device_id, name, trust_status, pinned_at, last_seen, last_addr, last_sync_at, capabilities FROM peers ORDER BY last_seen DESC NULLS LAST")
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        let peers = stmt
            .query_map([], |row| Ok(row_to_peer(row)))
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(peers)
    }

    pub fn list_trusted_peers(&self) -> Result<Vec<PeerRecord>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT device_id, name, trust_status, pinned_at, last_seen, last_addr, last_sync_at, capabilities FROM peers WHERE trust_status = 'trusted'")
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        let peers = stmt
            .query_map([], |row| Ok(row_to_peer(row)))
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(peers)
    }

    pub fn get_peer(&self, device_id: &str) -> Result<Option<PeerRecord>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT device_id, name, trust_status, pinned_at, last_seen, last_addr, last_sync_at, capabilities FROM peers WHERE device_id = ?1",
            params![device_id],
            |row| Ok(row_to_peer(row)),
        )
        .optional()
        .map_err(|e| ClipsterError::Database(e.to_string()))
    }

    pub fn peer_last_sync(&self, device_id: &str) -> Result<Option<DateTime<Utc>>, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let s: Option<String> = conn
            .query_row(
                "SELECT last_sync_at FROM peers WHERE device_id = ?1",
                params![device_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ClipsterError::Database(e.to_string()))?
            .flatten();
        Ok(s.map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .unwrap()
                .with_timezone(&Utc)
        }))
    }

    pub fn set_peer_last_sync(
        &self,
        device_id: &str,
        ts: DateTime<Utc>,
    ) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE peers SET last_sync_at = ?1 WHERE device_id = ?2",
            params![ts.to_rfc3339(), device_id],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
        Ok(())
    }
}

fn row_to_clip(row: &rusqlite::Row) -> Clip {
    let content_type_str: String = row.get_unwrap("content_type");
    let created_str: String = row.get_unwrap("created_at");
    let state_str: Option<String> = row.get_unwrap("state_modified_at");
    let id_str: String = row.get_unwrap("id");

    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .unwrap()
        .with_timezone(&chrono::Utc);
    let state_modified_at = state_str
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or(created_at);

    Clip {
        id: id_str.parse().unwrap(),
        content_type: content_type_str.parse().unwrap(),
        text_content: row.get_unwrap("text_content"),
        image_hash: row.get_unwrap("image_hash"),
        image_mime: row.get_unwrap("image_mime"),
        file_ref_path: row.get_unwrap("file_ref_path"),
        content_hash: row.get_unwrap("content_hash"),
        source_device: row.get_unwrap("source_device"),
        source_app: row.get_unwrap("source_app"),
        byte_size: row.get::<_, i64>("byte_size").unwrap() as u64,
        created_at,
        state_modified_at,
        is_favorite: row.get::<_, i32>("is_favorite").unwrap() != 0,
        is_deleted: row.get::<_, i32>("is_deleted").unwrap() != 0,
    }
}

fn row_to_peer(row: &rusqlite::Row) -> PeerRecord {
    let device_id: String = row.get_unwrap("device_id");
    let name: String = row.get_unwrap("name");
    let trust_status_str: String = row.get_unwrap("trust_status");
    let pinned_at: Option<String> = row.get_unwrap("pinned_at");
    let last_seen: Option<String> = row.get_unwrap("last_seen");
    let last_addr: Option<String> = row.get_unwrap("last_addr");
    let last_sync_at: Option<String> = row.get_unwrap("last_sync_at");
    let capabilities: Option<String> = row.get_unwrap("capabilities");

    fn parse_ts(s: Option<String>) -> Option<DateTime<Utc>> {
        s.and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
            .map(|d| d.with_timezone(&Utc))
    }

    PeerRecord {
        device_id,
        name,
        trust_status: TrustStatus::from_str(&trust_status_str).unwrap_or(TrustStatus::Pending),
        pinned_at: parse_ts(pinned_at),
        last_seen: parse_ts(last_seen),
        last_addr,
        last_sync_at: parse_ts(last_sync_at),
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipster_common::models::{ClipContentType, content_hash};

    fn make_test_clip(text: &str) -> Clip {
        let now = Utc::now();
        Clip {
            id: Uuid::now_v7(),
            content_type: ClipContentType::Text,
            text_content: Some(text.to_string()),
            image_hash: None,
            image_mime: None,
            file_ref_path: None,
            content_hash: content_hash(text.as_bytes()),
            source_device: "test-device".to_string(),
            source_app: None,
            byte_size: text.len() as u64,
            created_at: now,
            state_modified_at: now,
            is_favorite: false,
            is_deleted: false,
        }
    }

    fn setup_db() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.migrate().unwrap();
        db
    }

    #[test]
    fn migrations_apply_idempotently() {
        let db = Database::open(":memory:").unwrap();
        db.migrate().unwrap();
        db.migrate().unwrap();
        // schema_version table should have rows for 1 and 2
        let conn = db.conn.lock().unwrap();
        let max: u32 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(max, migrations::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn insert_and_get_round_trip() {
        let db = setup_db();
        let clip = make_test_clip("hello");
        db.insert_clip(&clip).unwrap();
        let fetched = db.get_clip(&clip.id).unwrap();
        assert_eq!(fetched.id, clip.id);
        assert_eq!(fetched.text_content, clip.text_content);
        assert_eq!(fetched.state_modified_at, clip.state_modified_at);
    }

    #[test]
    fn toggle_favorite_updates_state_modified_at() {
        let db = setup_db();
        let mut clip = make_test_clip("fav");
        clip.created_at = Utc::now() - chrono::Duration::hours(1);
        clip.state_modified_at = clip.created_at;
        db.insert_clip(&clip).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5));
        db.toggle_favorite(&clip.id).unwrap();

        let fetched = db.get_clip(&clip.id).unwrap();
        assert!(fetched.is_favorite);
        assert!(fetched.state_modified_at > clip.state_modified_at);
    }

    #[test]
    fn upsert_from_peer_inserts_new_clip() {
        let db = setup_db();
        let remote = make_test_clip("remote");
        db.upsert_clip_from_peer(&remote).unwrap();
        let fetched = db.get_clip(&remote.id).unwrap();
        assert_eq!(fetched.text_content, remote.text_content);
    }

    #[test]
    fn upsert_from_peer_lww_remote_newer_wins() {
        let db = setup_db();
        let mut local = make_test_clip("clip");
        local.state_modified_at = Utc::now() - chrono::Duration::hours(1);
        db.insert_clip(&local).unwrap();

        let mut remote = local.clone();
        remote.is_favorite = true;
        remote.state_modified_at = Utc::now();
        db.upsert_clip_from_peer(&remote).unwrap();

        let fetched = db.get_clip(&local.id).unwrap();
        assert!(fetched.is_favorite);
    }

    #[test]
    fn upsert_from_peer_lww_local_newer_wins() {
        let db = setup_db();
        let mut local = make_test_clip("clip");
        local.is_favorite = true;
        local.state_modified_at = Utc::now();
        db.insert_clip(&local).unwrap();

        let mut remote = local.clone();
        remote.is_favorite = false;
        remote.state_modified_at = Utc::now() - chrono::Duration::hours(1);
        db.upsert_clip_from_peer(&remote).unwrap();

        let fetched = db.get_clip(&local.id).unwrap();
        assert!(fetched.is_favorite); // local kept
    }

    #[test]
    fn list_clips_since_includes_tombstones() {
        let db = setup_db();
        let clip = make_test_clip("doomed");
        db.insert_clip(&clip).unwrap();
        let since = Utc::now() - chrono::Duration::hours(1);
        db.soft_delete(&clip.id).unwrap();

        let results = db.list_clips_since(since, 100).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_deleted);
    }

    #[test]
    fn peer_upsert_discovery_creates_pending() {
        let db = setup_db();
        let status = db
            .upsert_peer_discovery("fp123", "mac", "10.0.0.5:8743", None)
            .unwrap();
        assert_eq!(status, TrustStatus::Pending);

        let peer = db.get_peer("fp123").unwrap().unwrap();
        assert_eq!(peer.name, "mac");
        assert_eq!(peer.trust_status, TrustStatus::Pending);
    }

    #[test]
    fn peer_set_trust_flips_status() {
        let db = setup_db();
        db.upsert_peer_discovery("fp1", "mac", "10.0.0.5:8743", None).unwrap();
        db.set_peer_trust("fp1", TrustStatus::Trusted).unwrap();

        let peer = db.get_peer("fp1").unwrap().unwrap();
        assert_eq!(peer.trust_status, TrustStatus::Trusted);
        assert!(peer.pinned_at.is_some());

        let trusted = db.list_trusted_peers().unwrap();
        assert_eq!(trusted.len(), 1);
    }

    #[test]
    fn peer_last_sync_round_trip() {
        let db = setup_db();
        db.upsert_peer_discovery("fp1", "mac", "10.0.0.5:8743", None).unwrap();
        let now = Utc::now();
        db.set_peer_last_sync("fp1", now).unwrap();
        let got = db.peer_last_sync("fp1").unwrap().unwrap();
        // round-trip through rfc3339 may lose nanosecond precision; allow 1µs slack.
        assert!((got - now).num_milliseconds().abs() < 1);
    }
}
