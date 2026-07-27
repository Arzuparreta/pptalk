//! Optional store-and-forward node. It only stores opaque, client-encrypted envelopes.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_MAILBOX_QUOTA_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub database_path: PathBuf,
    pub max_envelope_bytes: usize,
    pub mailbox_quota_bytes: u64,
    pub max_ttl_seconds: u64,
}

impl NodeConfig {
    pub fn at(data_dir: impl AsRef<Path>) -> Self {
        Self {
            database_path: data_dir.as_ref().join("mailbox.sqlite3"),
            max_envelope_bytes: DEFAULT_MAX_ENVELOPE_BYTES,
            mailbox_quota_bytes: DEFAULT_MAILBOX_QUOTA_BYTES,
            max_ttl_seconds: DEFAULT_MAX_TTL_SECONDS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MailboxStore {
    config: NodeConfig,
}

impl MailboxStore {
    pub fn open(config: NodeConfig) -> Result<Self, NodeError> {
        if let Some(parent) = config.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { config };
        store.connection()?.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;
             CREATE TABLE IF NOT EXISTS mailbox_messages (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               capability BLOB NOT NULL,
               expires_unix INTEGER NOT NULL,
               envelope BLOB NOT NULL
             );
             CREATE INDEX IF NOT EXISTS mailbox_lookup
               ON mailbox_messages(capability, expires_unix, id);",
        )?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, NodeError> {
        Ok(Connection::open(&self.config.database_path)?)
    }

    pub fn deposit(
        &self,
        capability: &[u8; 32],
        envelope: &[u8],
        ttl_seconds: u64,
        now_unix: i64,
    ) -> Result<(), NodeError> {
        if envelope.is_empty() || envelope.len() > self.config.max_envelope_bytes {
            return Err(NodeError::EnvelopeSize);
        }
        let ttl = ttl_seconds.clamp(1, self.config.max_ttl_seconds);
        let expires = now_unix.saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX));
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM mailbox_messages WHERE expires_unix <= ?1",
            [now_unix],
        )?;
        let used: i64 = transaction.query_row(
            "SELECT COALESCE(SUM(length(envelope)), 0) FROM mailbox_messages WHERE capability = ?1",
            [capability.as_slice()],
            |row| row.get(0),
        )?;
        let new_total = u64::try_from(used.max(0)).unwrap_or(u64::MAX)
            + u64::try_from(envelope.len()).unwrap_or(u64::MAX);
        if new_total > self.config.mailbox_quota_bytes {
            return Err(NodeError::Quota);
        }
        transaction.execute(
            "INSERT INTO mailbox_messages(capability, expires_unix, envelope) VALUES (?1, ?2, ?3)",
            params![capability.as_slice(), expires, envelope],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomically drains the current batch, providing at-most-once mailbox delivery.
    /// Client event IDs make processing idempotent if a client retries after a lost response.
    pub fn drain(
        &self,
        capability: &[u8; 32],
        limit: usize,
        now_unix: i64,
    ) -> Result<Vec<Vec<u8>>, NodeError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM mailbox_messages WHERE expires_unix <= ?1",
            [now_unix],
        )?;
        let mut statement = transaction.prepare(
            "SELECT id, envelope FROM mailbox_messages
             WHERE capability = ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                capability.as_slice(),
                i64::try_from(limit.min(512)).unwrap_or(512)
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let batch = rows.collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        if let (Some(first), Some(last)) = (batch.first(), batch.last()) {
            transaction.execute(
                "DELETE FROM mailbox_messages WHERE capability = ?1 AND id BETWEEN ?2 AND ?3",
                params![capability.as_slice(), first.0, last.0],
            )?;
        }
        transaction.commit()?;
        Ok(batch.into_iter().map(|(_, bytes)| bytes).collect())
    }
}

#[derive(Debug, Deserialize)]
struct DepositQuery {
    ttl: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DrainQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct MailboxBatch {
    messages: Vec<String>,
}

pub fn router(store: MailboxStore) -> Router {
    let max = store.config.max_envelope_bytes;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/v1/mailboxes/{capability}/messages",
            post(deposit).get(drain),
        )
        .layer(DefaultBodyLimit::max(max))
        .with_state(Arc::new(store))
}

async fn deposit(
    State(store): State<Arc<MailboxStore>>,
    AxumPath(capability): AxumPath<String>,
    Query(query): Query<DepositQuery>,
    body: Bytes,
) -> Response {
    let capability = match parse_capability(&capability) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    match store.deposit(&capability, &body, query.ttl.unwrap_or(86_400), now_unix()) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(NodeError::Quota) => StatusCode::INSUFFICIENT_STORAGE.into_response(),
        Err(NodeError::EnvelopeSize) => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn drain(
    State(store): State<Arc<MailboxStore>>,
    AxumPath(capability): AxumPath<String>,
    Query(query): Query<DrainQuery>,
) -> Response {
    let capability = match parse_capability(&capability) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    match store.drain(&capability, query.limit.unwrap_or(128), now_unix()) {
        Ok(messages) => {
            use base64::{Engine, engine::general_purpose::STANDARD};
            let payload = MailboxBatch {
                messages: messages
                    .iter()
                    .map(|bytes| STANDARD.encode(bytes))
                    .collect(),
            };
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CACHE_CONTROL,
                "no-store".parse().expect("static header"),
            );
            (headers, axum::Json(payload)).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn parse_capability(value: &str) -> Result<[u8; 32], StatusCode> {
    let decoded = hex::decode(value).map_err(|_| StatusCode::NOT_FOUND)?;
    decoded.try_into().map_err(|_| StatusCode::NOT_FOUND)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("mailbox quota exceeded")]
    Quota,
    #[error("invalid envelope size")]
    EnvelopeSize,
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_store() -> (MailboxStore, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("pptalk-node-{}.sqlite3", uuid::Uuid::new_v4()));
        (
            MailboxStore::open(NodeConfig {
                database_path: path.clone(),
                ..NodeConfig::at(".")
            })
            .expect("store"),
            path,
        )
    }

    #[test]
    fn mailbox_is_capability_scoped_durable_and_draining() {
        let (store, path) = temporary_store();
        let alice = [1; 32];
        let bob = [2; 32];
        store
            .deposit(&alice, b"ciphertext", 60, 10)
            .expect("deposit");
        assert!(store.drain(&bob, 10, 11).expect("other mailbox").is_empty());
        drop(store);
        let store = MailboxStore::open(NodeConfig {
            database_path: path.clone(),
            ..NodeConfig::at(".")
        })
        .expect("reopen");
        assert_eq!(
            store.drain(&alice, 10, 11).expect("drain"),
            vec![b"ciphertext".to_vec()]
        );
        assert!(store.drain(&alice, 10, 11).expect("empty").is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn expired_messages_are_never_delivered() {
        let (store, path) = temporary_store();
        store
            .deposit(&[3; 32], b"ciphertext", 1, 10)
            .expect("deposit");
        assert!(store.drain(&[3; 32], 10, 11).expect("drain").is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn retention_is_capped_regardless_of_the_requested_ttl() {
        let (store, path) = temporary_store();
        let capability = [4; 32];
        store
            .deposit(&capability, b"ciphertext", u64::MAX, 0)
            .expect("deposit");
        let max_ttl = i64::try_from(DEFAULT_MAX_TTL_SECONDS).expect("ttl fits");
        assert!(
            store
                .drain(&capability, 10, max_ttl + 1)
                .expect("drain past the cap")
                .is_empty(),
            "a caller must not be able to hold storage beyond the retention limit"
        );

        store
            .deposit(&capability, b"ciphertext", u64::MAX, 0)
            .expect("deposit");
        assert_eq!(
            store
                .drain(&capability, 10, max_ttl - 1)
                .expect("drain within the cap")
                .len(),
            1
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_and_empty_envelopes_are_rejected() {
        let (store, path) = temporary_store();
        let capability = [5; 32];
        assert!(matches!(
            store.deposit(&capability, b"", 60, 10),
            Err(NodeError::EnvelopeSize)
        ));
        let oversized = vec![0_u8; DEFAULT_MAX_ENVELOPE_BYTES + 1];
        assert!(matches!(
            store.deposit(&capability, &oversized, 60, 10),
            Err(NodeError::EnvelopeSize)
        ));
        assert!(
            store.drain(&capability, 10, 11).expect("drain").is_empty(),
            "a rejected deposit must not consume storage"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_full_mailbox_rejects_deposits_without_touching_its_neighbours() {
        let path =
            std::env::temp_dir().join(format!("pptalk-node-{}.sqlite3", uuid::Uuid::new_v4()));
        let store = MailboxStore::open(NodeConfig {
            database_path: path.clone(),
            mailbox_quota_bytes: 32,
            ..NodeConfig::at(".")
        })
        .expect("store");
        let flooded = [6; 32];
        let neighbour = [7; 32];
        store
            .deposit(&flooded, &[0_u8; 24], 60, 10)
            .expect("first deposit fits");
        assert!(matches!(
            store.deposit(&flooded, &[0_u8; 24], 60, 10),
            Err(NodeError::Quota)
        ));
        // Quota is per capability: one noisy route must not deny service to another.
        store
            .deposit(&neighbour, &[0_u8; 24], 60, 10)
            .expect("neighbour is unaffected");
        assert_eq!(store.drain(&flooded, 10, 11).expect("drain").len(), 1);
        assert_eq!(store.drain(&neighbour, 10, 11).expect("drain").len(), 1);

        // Draining frees the quota again.
        store
            .deposit(&flooded, &[0_u8; 24], 60, 10)
            .expect("space is reclaimed after a drain");
        let _ = std::fs::remove_file(path);
    }
}
