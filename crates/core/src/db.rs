//! SQLite persistence. Most records are stored as JSON documents keyed by id;
//! the schema only indexes what we query on.

use crate::model::{Message, MessageDirection, RoomMessage};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS docs (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (kind, id)
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    direction TEXT NOT NULL,
    message TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS messages_user ON messages(username, timestamp);
CREATE TABLE IF NOT EXISTS share_cache (
    path TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    attrs TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS room_messages (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    room TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    username TEXT NOT NULL,
    message TEXT NOT NULL,
    is_self INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS room_messages_room ON room_messages(room, seq);
"#;

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) {
        let json = serde_json::to_string(value).expect("serializable");
        if let Err(e) = self.conn.lock().execute(
            "INSERT INTO docs(kind, id, json, updated_at) VALUES (?1, ?2, ?3, unixepoch())
             ON CONFLICT(kind, id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            params![kind, id, json],
        ) {
            tracing::error!("db put {kind}/{id}: {e}");
        }
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Option<T> {
        let json: Option<String> = self
            .conn
            .lock()
            .query_row("SELECT json FROM docs WHERE kind = ?1 AND id = ?2", params![kind, id], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        json.and_then(|j| serde_json::from_str(&j).ok())
    }

    pub fn all<T: DeserializeOwned>(&self, kind: &str) -> Vec<T> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached("SELECT json FROM docs WHERE kind = ?1 ORDER BY rowid").unwrap();
        let rows = stmt.query_map(params![kind], |r| r.get::<_, String>(0)).unwrap();
        rows.filter_map(|r| r.ok()).filter_map(|j| serde_json::from_str(&j).ok()).collect()
    }

    pub fn delete(&self, kind: &str, id: &str) {
        let _ = self.conn.lock().execute("DELETE FROM docs WHERE kind = ?1 AND id = ?2", params![kind, id]);
    }

    pub fn delete_kind(&self, kind: &str) {
        let _ = self.conn.lock().execute("DELETE FROM docs WHERE kind = ?1", params![kind]);
    }

    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.conn.lock().query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn kv_set(&self, key: &str, value: &str) {
        let _ = self.conn.lock().execute(
            "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
    }

    // --- room messages ---

    /// Log a room message, keeping at most `keep` per room.
    pub fn insert_room_message(&self, room: &str, m: &RoomMessage, keep: usize) {
        let conn = self.conn.lock();
        let r = conn.execute(
            "INSERT INTO room_messages(room, timestamp, username, message, is_self) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![room, m.timestamp.to_rfc3339(), m.username, m.message, m.is_self as i32],
        );
        // Prune now and then rather than on every insert.
        if let Ok(1) = r {
            if conn.last_insert_rowid() % 200 == 0 {
                let _ = conn.execute(
                    "DELETE FROM room_messages WHERE room = ?1 AND seq <= (SELECT seq FROM room_messages WHERE room = ?1 ORDER BY seq DESC LIMIT 1 OFFSET ?2)",
                    params![room, keep as i64],
                );
            }
        }
    }

    /// The newest `limit` messages of a room, oldest first.
    pub fn room_messages(&self, room: &str, limit: usize) -> Vec<RoomMessage> {
        let conn = self.conn.lock();
        let Ok(mut stmt) = conn.prepare_cached(
            "SELECT timestamp, username, message, is_self FROM (SELECT * FROM room_messages WHERE room = ?1 ORDER BY seq DESC LIMIT ?2) ORDER BY seq",
        ) else {
            return vec![];
        };
        stmt.query_map(params![room, limit as i64], |r| {
            let ts: String = r.get(0)?;
            Ok(RoomMessage {
                timestamp: chrono::DateTime::parse_from_rfc3339(&ts).map(|t| t.to_utc()).unwrap_or_default(),
                username: r.get(1)?,
                message: r.get(2)?,
                is_self: r.get::<_, i32>(3)? != 0,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    // --- private messages ---

    pub fn insert_message(&self, m: &Message, read: bool) {
        let dir = match m.direction {
            MessageDirection::In => "in",
            MessageDirection::Out => "out",
        };
        let _ = self.conn.lock().execute(
            "INSERT OR IGNORE INTO messages(id, username, timestamp, direction, message, read) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![m.id, m.username, m.timestamp.to_rfc3339(), dir, m.message, read as i32],
        );
    }

    fn row_to_message(r: &rusqlite::Row) -> rusqlite::Result<Message> {
        let ts: String = r.get(2)?;
        let dir: String = r.get(3)?;
        Ok(Message {
            id: r.get(0)?,
            username: r.get(1)?,
            timestamp: chrono::DateTime::parse_from_rfc3339(&ts).map(|t| t.to_utc()).unwrap_or_default(),
            direction: if dir == "in" { MessageDirection::In } else { MessageDirection::Out },
            message: r.get(4)?,
        })
    }

    pub fn messages(&self, username: &str, limit: usize) -> Vec<Message> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, username, timestamp, direction, message FROM
                 (SELECT * FROM messages WHERE username = ?1 ORDER BY timestamp DESC LIMIT ?2) ORDER BY timestamp",
            )
            .unwrap();
        stmt.query_map(params![username, limit as i64], Self::row_to_message).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// (username, unread, last message) for every conversation.
    pub fn conversations(&self) -> Vec<(String, u32, Option<Message>)> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare_cached(
                "SELECT username, SUM(CASE WHEN read = 0 AND direction = 'in' THEN 1 ELSE 0 END), MAX(timestamp)
                 FROM messages GROUP BY username ORDER BY MAX(timestamp) DESC",
            )
            .unwrap();
        let users: Vec<(String, u32)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().filter_map(|r| r.ok()).collect();
        let mut last = conn
            .prepare_cached(
                "SELECT id, username, timestamp, direction, message FROM messages WHERE username = ?1 ORDER BY timestamp DESC LIMIT 1",
            )
            .unwrap();
        users
            .into_iter()
            .map(|(u, unread)| {
                let m = last.query_row(params![u], Self::row_to_message).ok();
                (u, unread, m)
            })
            .collect()
    }

    pub fn conversation_users(&self) -> Vec<String> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached("SELECT DISTINCT username FROM messages").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn mark_read(&self, username: &str) {
        let _ = self.conn.lock().execute("UPDATE messages SET read = 1 WHERE username = ?1", params![username]);
    }

    pub fn unread_total(&self) -> u32 {
        self.conn
            .lock()
            .query_row("SELECT COUNT(*) FROM messages WHERE read = 0 AND direction = 'in'", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn delete_conversation(&self, username: &str) {
        let _ = self.conn.lock().execute("DELETE FROM messages WHERE username = ?1", params![username]);
    }

    // --- share metadata cache ---

    pub fn share_cache_load(&self) -> std::collections::HashMap<String, (u64, i64, String)> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT path, size, mtime, attrs FROM share_cache").unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? as u64, r.get(2)?, r.get(3)?))))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn share_cache_store(&self, rows: &[(String, u64, i64, String)]) {
        let mut conn = self.conn.lock();
        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(e) => return tracing::error!("share cache: {e}"),
        };
        let _ = tx.execute("DELETE FROM share_cache", []);
        {
            let mut stmt = tx.prepare("INSERT OR REPLACE INTO share_cache(path, size, mtime, attrs) VALUES (?1, ?2, ?3, ?4)").unwrap();
            for (p, s, m, a) in rows {
                let _ = stmt.execute(params![p, *s as i64, m, a]);
            }
        }
        let _ = tx.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_log_roundtrip() {
        let db = Db::in_memory().unwrap();
        for i in 0..5 {
            let m = RoomMessage { timestamp: chrono::Utc::now(), username: format!("u{i}"), message: format!("m{i}"), is_self: i == 2 };
            db.insert_room_message("r", &m, 100);
        }
        db.insert_room_message("other", &RoomMessage { timestamp: chrono::Utc::now(), username: "x".into(), message: "y".into(), is_self: false }, 100);
        let got = db.room_messages("r", 3);
        assert_eq!(got.iter().map(|m| m.message.as_str()).collect::<Vec<_>>(), ["m2", "m3", "m4"]);
        assert!(got[0].is_self);
    }

    #[test]
    fn docs_and_messages() {
        let db = Db::in_memory().unwrap();
        db.put("t", "1", &serde_json::json!({"a": 1}));
        db.put("t", "1", &serde_json::json!({"a": 2}));
        let all: Vec<serde_json::Value> = db.all("t");
        assert_eq!(all, vec![serde_json::json!({"a": 2})]);
        let m = Message {
            id: "m1".into(),
            timestamp: chrono::Utc::now(),
            username: "bob".into(),
            direction: MessageDirection::In,
            message: "hi".into(),
        };
        db.insert_message(&m, false);
        assert_eq!(db.unread_total(), 1);
        assert_eq!(db.conversations()[0].1, 1);
        db.mark_read("bob");
        assert_eq!(db.unread_total(), 0);
        assert_eq!(db.messages("bob", 10).len(), 1);
    }
}
