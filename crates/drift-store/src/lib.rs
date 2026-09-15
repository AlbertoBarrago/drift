use std::path::Path;

use chrono::{DateTime, Utc};
use drift_domain::SystemSnapshot;
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("snapshot encoding error: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("snapshot decoding error: {0}")]
    Decode(#[from] bincode::error::DecodeError),
    #[error("compression error: {0}")]
    Compression(#[from] std::io::Error),
}

pub struct SnapshotStore {
    connection: Connection,
}

impl SnapshotStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "\
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS snapshots (
              id TEXT PRIMARY KEY,
              host_scope_id TEXT NOT NULL,
              completed_at TEXT NOT NULL,
              content_hash TEXT NOT NULL,
              payload BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tags (
              name TEXT PRIMARY KEY,
              snapshot_id TEXT NOT NULL REFERENCES snapshots(id)
            );
        ",
        )?;
        Ok(Self { connection })
    }

    pub fn save(&self, snapshot: &SystemSnapshot) -> Result<(), StoreError> {
        let encoded = bincode::serde::encode_to_vec(snapshot, bincode::config::standard())?;
        let payload = zstd::stream::encode_all(encoded.as_slice(), 3)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO snapshots (id, host_scope_id, completed_at, content_hash, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![snapshot.id, snapshot.host_scope_id, snapshot.completed_at.to_rfc3339(), snapshot.content_hash, payload],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<SystemSnapshot>, StoreError> {
        let payload: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT payload FROM snapshots WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        payload.map(|payload| Self::decode(&payload)).transpose()
    }

    pub fn latest(&self) -> Result<Option<SystemSnapshot>, StoreError> {
        let payload: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT payload FROM snapshots ORDER BY completed_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        payload.map(|payload| Self::decode(&payload)).transpose()
    }

    pub fn all(&self) -> Result<Vec<SystemSnapshot>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM snapshots ORDER BY completed_at ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|row| Self::decode(&row?))
            .collect::<Result<Vec<_>, StoreError>>()
    }

    pub fn list(&self) -> Result<Vec<(String, DateTime<Utc>, String)>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id, completed_at, content_hash FROM snapshots ORDER BY completed_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let timestamp: String = row.get(1)?;
            Ok((row.get(0)?, timestamp, row.get(2)?))
        })?;
        rows.map(|row| {
            let (id, timestamp, hash) = row?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp)
                .expect("stored timestamps are RFC3339")
                .with_timezone(&Utc);
            Ok((id, timestamp, hash))
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()
        .map_err(StoreError::from)
    }

    pub fn tag(&self, name: &str, snapshot_id: &str) -> Result<(), StoreError> {
        self.connection.execute("INSERT INTO tags (name, snapshot_id) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET snapshot_id = excluded.snapshot_id", params![name, snapshot_id])?;
        Ok(())
    }

    pub fn tagged(&self, name: &str) -> Result<Option<SystemSnapshot>, StoreError> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT snapshot_id FROM tags WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| self.get(&id)).transpose().map(Option::flatten)
    }

    fn decode(payload: &[u8]) -> Result<SystemSnapshot, StoreError> {
        let encoded = zstd::stream::decode_all(payload)?;
        let (snapshot, _) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard())?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use drift_domain::SnapshotFacts;

    use super::*;

    #[test]
    fn stores_and_retrieves_a_snapshot() {
        let path = std::env::temp_dir().join(format!("drift-store-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = SnapshotStore::open(&path).expect("open store");
        let snapshot = SystemSnapshot::new(
            "test-host".into(),
            Utc::now(),
            Utc::now(),
            vec![],
            SnapshotFacts::default(),
        );
        store.save(&snapshot).expect("save snapshot");
        assert_eq!(
            store.get(&snapshot.id).expect("read snapshot"),
            Some(snapshot)
        );
        drop(store);
        std::fs::remove_file(path).expect("remove test store");
    }
}
