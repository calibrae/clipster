use clipster_common::error::ClipsterError;
use clipster_common::models::{Clip, ClipListQuery};
use rusqlite::{params, Connection};
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

    pub fn insert_clip(&self, clip: &Clip) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clips (id, content_type, text_content, image_hash, image_mime, file_ref_path, content_hash, source_device, source_app, byte_size, created_at, is_favorite, is_deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                clip.is_favorite as i32,
                clip.is_deleted as i32,
            ],
        )
        .map_err(|e| ClipsterError::Database(e.to_string()))?;
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
        let offset = query.offset.unwrap_or(0).min(100_000);
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

    pub fn soft_delete(&self, id: &Uuid) -> Result<(), ClipsterError> {
        let conn = self.conn.lock().unwrap();
        let affected = conn
            .execute(
                "UPDATE clips SET is_deleted = 1 WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(|e| ClipsterError::Database(e.to_string()))?;
        if affected == 0 {
            return Err(ClipsterError::NotFound(format!("clip {id}")));
        }
        Ok(())
    }

    pub fn toggle_favorite(&self, id: &Uuid) -> Result<bool, ClipsterError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET is_favorite = 1 - is_favorite WHERE id = ?1 AND is_deleted = 0",
            params![id.to_string()],
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
}

fn row_to_clip(row: &rusqlite::Row) -> Clip {
    let content_type_str: String = row.get_unwrap("content_type");
    let created_str: String = row.get_unwrap("created_at");
    let id_str: String = row.get_unwrap("id");

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
        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
            .unwrap()
            .with_timezone(&chrono::Utc),
        is_favorite: row.get::<_, i32>("is_favorite").unwrap() != 0,
        is_deleted: row.get::<_, i32>("is_deleted").unwrap() != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipster_common::models::{ClipContentType, content_hash};
    use chrono::Utc;

    fn make_test_clip(text: &str) -> Clip {
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
            created_at: Utc::now(),
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
    fn open_and_migrate_succeeds() {
        let db = Database::open(":memory:").unwrap();
        db.migrate().unwrap();
    }

    #[test]
    fn insert_and_get_clip_round_trip() {
        let db = setup_db();
        let clip = make_test_clip("hello world");
        db.insert_clip(&clip).unwrap();

        let fetched = db.get_clip(&clip.id).unwrap();
        assert_eq!(fetched.id, clip.id);
        assert_eq!(fetched.content_type, clip.content_type);
        assert_eq!(fetched.text_content, clip.text_content);
        assert_eq!(fetched.content_hash, clip.content_hash);
        assert_eq!(fetched.source_device, clip.source_device);
        assert_eq!(fetched.byte_size, clip.byte_size);
        assert_eq!(fetched.is_favorite, false);
        assert_eq!(fetched.is_deleted, false);
    }

    #[test]
    fn get_clip_not_found_for_nonexistent_id() {
        let db = setup_db();
        let id = Uuid::now_v7();
        let result = db.get_clip(&id);
        assert!(result.is_err());
        match result.unwrap_err() {
            ClipsterError::NotFound(msg) => assert!(msg.contains(&id.to_string())),
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn list_clips_empty_db_returns_zero() {
        let db = setup_db();
        let query = ClipListQuery {
            limit: None,
            offset: None,
            content_type: None,
            search: None,
            device: None,
            since: None,
            exclude_device: None,
        };
        let (clips, total) = db.list_clips(&query).unwrap();
        assert_eq!(clips.len(), 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn list_clips_returns_reverse_chronological_order() {
        let db = setup_db();

        let mut clip1 = make_test_clip("first");
        clip1.created_at = Utc::now() - chrono::Duration::seconds(10);
        db.insert_clip(&clip1).unwrap();

        let mut clip2 = make_test_clip("second");
        clip2.created_at = Utc::now() - chrono::Duration::seconds(5);
        db.insert_clip(&clip2).unwrap();

        let mut clip3 = make_test_clip("third");
        clip3.created_at = Utc::now();
        db.insert_clip(&clip3).unwrap();

        let query = ClipListQuery {
            limit: None,
            offset: None,
            content_type: None,
            search: None,
            device: None,
            since: None,
            exclude_device: None,
        };
        let (clips, total) = db.list_clips(&query).unwrap();
        assert_eq!(total, 3);
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[0].id, clip3.id);
        assert_eq!(clips[1].id, clip2.id);
        assert_eq!(clips[2].id, clip1.id);
    }

    #[test]
    fn list_clips_search_filter_matches_text_content() {
        let db = setup_db();
        db.insert_clip(&make_test_clip("rust programming")).unwrap();
        db.insert_clip(&make_test_clip("python scripting")).unwrap();

        let query = ClipListQuery {
            limit: None,
            offset: None,
            content_type: None,
            search: Some("rust".to_string()),
            device: None,
            since: None,
            exclude_device: None,
        };
        let (clips, total) = db.list_clips(&query).unwrap();
        assert_eq!(total, 1);
        assert_eq!(clips[0].text_content.as_deref(), Some("rust programming"));
    }

    #[test]
    fn list_clips_device_filter() {
        let db = setup_db();

        let mut clip1 = make_test_clip("from mac");
        clip1.source_device = "mac".to_string();
        db.insert_clip(&clip1).unwrap();

        let mut clip2 = make_test_clip("from linux");
        clip2.source_device = "linux".to_string();
        db.insert_clip(&clip2).unwrap();

        let query = ClipListQuery {
            limit: None,
            offset: None,
            content_type: None,
            search: None,
            device: Some("linux".to_string()),
            since: None,
            exclude_device: None,
        };
        let (clips, total) = db.list_clips(&query).unwrap();
        assert_eq!(total, 1);
        assert_eq!(clips[0].source_device, "linux");
    }

    #[test]
    fn list_clips_limit_offset_pagination() {
        let db = setup_db();
        for i in 0..5 {
            let mut clip = make_test_clip(&format!("clip {i}"));
            clip.created_at = Utc::now() - chrono::Duration::seconds(10 - i);
            db.insert_clip(&clip).unwrap();
        }

        // First page: limit 2, offset 0
        let query = ClipListQuery {
            limit: Some(2),
            offset: Some(0),
            content_type: None,
            search: None,
            device: None,
            since: None,
            exclude_device: None,
        };
        let (clips, total) = db.list_clips(&query).unwrap();
        assert_eq!(total, 5);
        assert_eq!(clips.len(), 2);

        // Second page: limit 2, offset 2
        let query = ClipListQuery {
            limit: Some(2),
            offset: Some(2),
            content_type: None,
            search: None,
            device: None,
            since: None,
            exclude_device: None,
        };
        let (clips2, total2) = db.list_clips(&query).unwrap();
        assert_eq!(total2, 5);
        assert_eq!(clips2.len(), 2);

        // Pages should not overlap
        assert_ne!(clips[0].id, clips2[0].id);
        assert_ne!(clips[1].id, clips2[1].id);
    }

    #[test]
    fn soft_delete_marks_clip_and_hides_from_get() {
        let db = setup_db();
        let clip = make_test_clip("to delete");
        db.insert_clip(&clip).unwrap();

        db.soft_delete(&clip.id).unwrap();

        let result = db.get_clip(&clip.id);
        assert!(result.is_err());
        match result.unwrap_err() {
            ClipsterError::NotFound(_) => {}
            other => panic!("expected NotFound, got: {other:?}"),
        }
    }

    #[test]
    fn toggle_favorite_toggles_and_returns_correct_value() {
        let db = setup_db();
        let clip = make_test_clip("fav test");
        db.insert_clip(&clip).unwrap();

        let fav = db.toggle_favorite(&clip.id).unwrap();
        assert!(fav);

        let fav = db.toggle_favorite(&clip.id).unwrap();
        assert!(!fav);

        let fav = db.toggle_favorite(&clip.id).unwrap();
        assert!(fav);
    }

    #[test]
    fn has_recent_duplicate_returns_true_for_same_hash() {
        let db = setup_db();
        let clip = make_test_clip("duplicate me");
        db.insert_clip(&clip).unwrap();

        let is_dup = db.has_recent_duplicate(&clip.content_hash, 60).unwrap();
        assert!(is_dup);
    }

    #[test]
    fn has_recent_duplicate_returns_false_for_different_hash() {
        let db = setup_db();
        let clip = make_test_clip("something");
        db.insert_clip(&clip).unwrap();

        let other_hash = content_hash(b"completely different");
        let is_dup = db.has_recent_duplicate(&other_hash, 60).unwrap();
        assert!(!is_dup);
    }
}
