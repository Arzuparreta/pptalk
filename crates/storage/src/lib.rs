//! SQLCipher-backed local persistence.

use std::path::Path;

use pptalk_core::IdentityEvent;
use pptalk_protocol::{
    CausalFrontier, ConversationEvent, ConversationId, DeviceId, EventId, IdentityId, WireDecode,
    WireEncode,
};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

const SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DatabaseKey([u8; 32]);

impl std::fmt::Debug for DatabaseKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseKey([REDACTED])")
    }
}

impl DatabaseKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn generate() -> Self {
        use rand::RngCore;
        let mut bytes = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub const fn expose_for_profile(&self) -> [u8; 32] {
        self.0
    }
}

#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>, key: &DatabaseKey) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        Self::configure(connection, key)
    }

    pub fn in_memory(key: &DatabaseKey) -> Result<Self, StorageError> {
        Self::configure(Connection::open_in_memory()?, key)
    }

    fn configure(connection: Connection, key: &DatabaseKey) -> Result<Self, StorageError> {
        connection.execute_batch(&format!(
            "PRAGMA key = \"x'{}'\";\
             PRAGMA cipher_memory_security = ON;\
             PRAGMA foreign_keys = ON;\
             PRAGMA journal_mode = WAL;\
             PRAGMA synchronous = NORMAL;\
             PRAGMA busy_timeout = 5000;",
            hex::encode(key.0)
        ))?;
        let cipher_version: Option<String> = connection
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .optional()?;
        if cipher_version.as_deref().unwrap_or_default().is_empty() {
            return Err(StorageError::CipherUnavailable);
        }
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StorageError> {
        self.connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS schema_meta (
               singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
               version INTEGER NOT NULL
             );
             INSERT OR IGNORE INTO schema_meta(singleton, version) VALUES (1, 0);

             CREATE TABLE IF NOT EXISTS identity_events (
               identity_id BLOB NOT NULL,
               sequence INTEGER NOT NULL,
               event BLOB NOT NULL,
               PRIMARY KEY(identity_id, sequence)
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS conversations (
               conversation_id BLOB PRIMARY KEY,
               owner_identity BLOB NOT NULL,
               title TEXT NOT NULL,
               created_at_ms INTEGER NOT NULL,
               archived INTEGER NOT NULL DEFAULT 0
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS conversation_events (
               conversation_id BLOB NOT NULL,
               event_id BLOB NOT NULL,
               author_device BLOB NOT NULL,
               device_sequence INTEGER NOT NULL,
               logical_time_ms INTEGER NOT NULL,
               event BLOB NOT NULL,
               PRIMARY KEY(conversation_id, event_id),
               UNIQUE(conversation_id, author_device, device_sequence),
               FOREIGN KEY(conversation_id) REFERENCES conversations(conversation_id) ON DELETE CASCADE
             ) WITHOUT ROWID;

             CREATE INDEX IF NOT EXISTS conversation_events_timeline
             ON conversation_events(conversation_id, logical_time_ms, author_device, device_sequence);

             CREATE TABLE IF NOT EXISTS outbox (
               conversation_id BLOB NOT NULL,
               event_id BLOB NOT NULL,
               recipient_device BLOB NOT NULL,
               envelope BLOB NOT NULL,
               attempts INTEGER NOT NULL DEFAULT 0,
               next_attempt_ms INTEGER NOT NULL,
               PRIMARY KEY(event_id, recipient_device)
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS consumed_invites (
               secret_hash BLOB PRIMARY KEY,
               consumed_at_ms INTEGER NOT NULL
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS blobs (
               ciphertext_hash BLOB PRIMARY KEY,
               manifest BLOB NOT NULL,
               local_path TEXT,
               pinned INTEGER NOT NULL DEFAULT 0,
               last_access_ms INTEGER NOT NULL
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS mls_states (
               conversation_id BLOB PRIMARY KEY,
               snapshot BLOB NOT NULL,
               updated_at_ms INTEGER NOT NULL
             ) WITHOUT ROWID;

             CREATE TABLE IF NOT EXISTS direct_messages (
               message_id BLOB PRIMARY KEY,
               peer_identity BLOB NOT NULL,
               sender_name TEXT NOT NULL,
               body TEXT NOT NULL,
               sent_at_unix INTEGER NOT NULL,
               outgoing INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS direct_messages_timeline
               ON direct_messages(peer_identity, sent_at_unix, message_id);

             UPDATE schema_meta SET version = 1 WHERE singleton = 1;
             COMMIT;",
        )?;
        let version: i64 = self.connection.query_row(
            "SELECT version FROM schema_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if version != SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchema(version));
        }
        Ok(())
    }

    pub fn save_identity_event(
        &self,
        identity: IdentityId,
        event: &IdentityEvent,
    ) -> Result<(), StorageError> {
        let sequence = i64::try_from(event.sequence).map_err(|_| StorageError::IntegerOverflow)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO identity_events(identity_id, sequence, event) VALUES (?1, ?2, ?3)",
            params![identity.as_bytes().as_slice(), sequence, event.to_wire()?],
        )?;
        Ok(())
    }

    pub fn load_identity_events(
        &self,
        identity: IdentityId,
    ) -> Result<Vec<IdentityEvent>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT event FROM identity_events WHERE identity_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([identity.as_bytes().as_slice()], |row| {
            row.get::<_, Vec<u8>>(0)
        })?;
        rows.map(|row| IdentityEvent::from_wire(&row?).map_err(StorageError::Codec))
            .collect()
    }

    pub fn create_conversation(
        &self,
        id: ConversationId,
        owner: IdentityId,
        title: &str,
        created_at_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO conversations(conversation_id, owner_identity, title, created_at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![id.as_bytes().as_slice(), owner.as_bytes().as_slice(), title, created_at_ms],
        )?;
        Ok(())
    }

    pub fn save_event(&self, event: &ConversationEvent) -> Result<bool, StorageError> {
        let device_sequence =
            i64::try_from(event.device_sequence).map_err(|_| StorageError::IntegerOverflow)?;
        let changed = self.connection.execute(
            "INSERT OR IGNORE INTO conversation_events(
                conversation_id, event_id, author_device, device_sequence, logical_time_ms, event
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.conversation_id.as_bytes().as_slice(),
                event.event_id.as_bytes().as_slice(),
                event.author_device.as_bytes().as_slice(),
                device_sequence,
                event.logical_time_ms,
                event.to_wire()?,
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn load_events(&self, id: ConversationId) -> Result<Vec<ConversationEvent>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT event FROM conversation_events
             WHERE conversation_id = ?1
             ORDER BY logical_time_ms, author_device, device_sequence",
        )?;
        let rows =
            statement.query_map([id.as_bytes().as_slice()], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|row| ConversationEvent::from_wire(&row?).map_err(StorageError::Codec))
            .collect()
    }

    /// Returns every event the remote causal frontier does not yet cover.
    pub fn events_after(
        &self,
        id: ConversationId,
        frontier: &CausalFrontier,
    ) -> Result<Vec<ConversationEvent>, StorageError> {
        Ok(self
            .load_events(id)?
            .into_iter()
            .filter(|event| {
                event.device_sequence
                    > frontier
                        .get(&event.author_device)
                        .copied()
                        .unwrap_or_default()
            })
            .collect())
    }

    pub fn enqueue(
        &self,
        conversation_id: ConversationId,
        event_id: EventId,
        recipient: DeviceId,
        envelope: &[u8],
        next_attempt_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO outbox(
                conversation_id, event_id, recipient_device, envelope, attempts, next_attempt_ms
             ) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![
                conversation_id.as_bytes().as_slice(),
                event_id.as_bytes().as_slice(),
                recipient.as_bytes().as_slice(),
                envelope,
                next_attempt_ms,
            ],
        )?;
        Ok(())
    }

    pub fn due_outbox(&self, now_ms: i64, limit: usize) -> Result<Vec<OutboxItem>, StorageError> {
        let limit = i64::try_from(limit).map_err(|_| StorageError::IntegerOverflow)?;
        let mut statement = self.connection.prepare(
            "SELECT conversation_id, event_id, recipient_device, envelope, attempts
             FROM outbox WHERE next_attempt_ms <= ?1 ORDER BY next_attempt_ms LIMIT ?2",
        )?;
        let rows = statement.query_map(params![now_ms, limit], |row| {
            Ok(OutboxItem {
                conversation_id: id_from_blob(row.get_ref(0)?.as_blob()?),
                event_id: event_id_from_blob(row.get_ref(1)?.as_blob()?),
                recipient: device_id_from_blob(row.get_ref(2)?.as_blob()?),
                envelope: row.get(3)?,
                attempts: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    pub fn acknowledge(&self, event_id: EventId, recipient: DeviceId) -> Result<(), StorageError> {
        self.connection.execute(
            "DELETE FROM outbox WHERE event_id = ?1 AND recipient_device = ?2",
            params![
                event_id.as_bytes().as_slice(),
                recipient.as_bytes().as_slice()
            ],
        )?;
        Ok(())
    }

    pub fn defer_outbox(
        &self,
        event_id: EventId,
        recipient: DeviceId,
        next_attempt_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "UPDATE outbox SET attempts = attempts + 1, next_attempt_ms = ?3
             WHERE event_id = ?1 AND recipient_device = ?2",
            params![
                event_id.as_bytes().as_slice(),
                recipient.as_bytes().as_slice(),
                next_attempt_ms
            ],
        )?;
        Ok(())
    }

    pub fn consume_invite(&self, secret: &[u8; 32], now_ms: i64) -> Result<bool, StorageError> {
        let hash = blake3::hash(secret);
        let changed = self.connection.execute(
            "INSERT OR IGNORE INTO consumed_invites(secret_hash, consumed_at_ms) VALUES (?1, ?2)",
            params![hash.as_bytes().as_slice(), now_ms],
        )?;
        Ok(changed == 1)
    }

    pub fn save_mls_state(
        &self,
        conversation_id: ConversationId,
        snapshot: &[u8],
        updated_at_ms: i64,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO mls_states(conversation_id, snapshot, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation_id) DO UPDATE SET
               snapshot = excluded.snapshot,
               updated_at_ms = excluded.updated_at_ms",
            params![
                conversation_id.as_bytes().as_slice(),
                snapshot,
                updated_at_ms
            ],
        )?;
        Ok(())
    }

    pub fn load_mls_state(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.connection
            .query_row(
                "SELECT snapshot FROM mls_states WHERE conversation_id = ?1",
                [conversation_id.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Sqlite)
    }

    pub fn save_direct_message(&self, message: &DirectMessageRecord) -> Result<bool, StorageError> {
        let changed = self.connection.execute(
            "INSERT OR IGNORE INTO direct_messages(
               message_id, peer_identity, sender_name, body, sent_at_unix, outgoing
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                message.message_id.as_slice(),
                message.peer_identity.as_bytes().as_slice(),
                message.sender_name,
                message.body,
                message.sent_at_unix,
                message.outgoing,
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn load_direct_messages(
        &self,
        peer_identity: IdentityId,
        limit: usize,
    ) -> Result<Vec<DirectMessageRecord>, StorageError> {
        let limit = i64::try_from(limit).map_err(|_| StorageError::IntegerOverflow)?;
        let mut statement = self.connection.prepare(
            "SELECT message_id, peer_identity, sender_name, body, sent_at_unix, outgoing
             FROM direct_messages WHERE peer_identity = ?1
             ORDER BY sent_at_unix DESC, message_id DESC LIMIT ?2",
        )?;
        let rows =
            statement.query_map(params![peer_identity.as_bytes().as_slice(), limit], |row| {
                Ok(DirectMessageRecord {
                    message_id: bytes_32(row.get_ref(0)?.as_blob()?),
                    peer_identity: identity_id_from_blob(row.get_ref(1)?.as_blob()?),
                    sender_name: row.get(2)?,
                    body: row.get(3)?,
                    sent_at_unix: row.get(4)?,
                    outgoing: row.get(5)?,
                })
            })?;
        let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
        messages.reverse();
        Ok(messages)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectMessageRecord {
    pub message_id: [u8; 32],
    pub peer_identity: IdentityId,
    pub sender_name: String,
    pub body: String,
    pub sent_at_unix: i64,
    pub outgoing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub conversation_id: ConversationId,
    pub event_id: EventId,
    pub recipient: DeviceId,
    pub envelope: Vec<u8>,
    pub attempts: u32,
}

fn bytes_32(blob: &[u8]) -> [u8; 32] {
    let mut bytes = [0; 32];
    if blob.len() == 32 {
        bytes.copy_from_slice(blob);
    }
    bytes
}

fn id_from_blob(blob: &[u8]) -> ConversationId {
    ConversationId::from_bytes(bytes_32(blob))
}

fn event_id_from_blob(blob: &[u8]) -> EventId {
    EventId::from_bytes(bytes_32(blob))
}

fn device_id_from_blob(blob: &[u8]) -> DeviceId {
    DeviceId::from_bytes(bytes_32(blob))
}

fn identity_id_from_blob(blob: &[u8]) -> IdentityId {
    IdentityId::from_bytes(bytes_32(blob))
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLCipher support is unavailable in this build")]
    CipherUnavailable,
    #[error("database schema {0} is newer or unsupported")]
    UnsupportedSchema(i64),
    #[error("database operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("wire codec failed: {0}")]
    Codec(#[from] pptalk_protocol::CodecError),
    #[error("integer is too large for SQLite")]
    IntegerOverflow,
}

#[cfg(test)]
mod tests {
    use pptalk_core::{ConversationBuilder, DeviceKeyPair, IdentityLog};
    use pptalk_protocol::{CausalFrontier, EventBody, MessageContent};
    use rand::rngs::OsRng;

    use super::*;

    #[test]
    fn persists_identity_events_and_idempotent_conversation_events() {
        let store = Store::in_memory(&DatabaseKey::from_bytes([7; 32])).expect("store");
        let key = DeviceKeyPair::generate(&mut OsRng);
        let identity = IdentityLog::create(&key, "desktop", 1).expect("identity");
        store
            .save_identity_event(identity.identity_id(), &identity.events()[0])
            .expect("save identity");
        assert_eq!(
            store
                .load_identity_events(identity.identity_id())
                .expect("load")
                .len(),
            1
        );

        let conversation_id = ConversationId::from_bytes([8; 32]);
        store
            .create_conversation(conversation_id, identity.identity_id(), "friends", 1)
            .expect("conversation");
        let mut builder = ConversationBuilder::new(
            conversation_id,
            identity.identity_id(),
            key.device_id(),
            CausalFrontier::new(),
        );
        let event = builder
            .build(
                EventBody::MessageCreate {
                    content: MessageContent {
                        text: "hello".into(),
                        reply_to: None,
                        attachment_ids: vec![],
                    },
                },
                2,
                &mut OsRng,
            )
            .expect("event");
        assert!(store.save_event(&event).expect("first insert"));
        assert!(!store.save_event(&event).expect("duplicate insert"));
        assert_eq!(
            store.load_events(conversation_id).expect("events"),
            vec![event]
        );
    }

    #[test]
    fn invite_consumption_is_atomic() {
        let store = Store::in_memory(&DatabaseKey::from_bytes([1; 32])).expect("store");
        assert!(store.consume_invite(&[9; 32], 1).expect("first"));
        assert!(!store.consume_invite(&[9; 32], 2).expect("replay"));
    }

    #[test]
    fn stores_mls_snapshots_inside_the_encrypted_database() {
        let store = Store::in_memory(&DatabaseKey::from_bytes([2; 32])).expect("store");
        let id = ConversationId::from_bytes([3; 32]);
        assert!(store.load_mls_state(id).expect("empty").is_none());
        store
            .save_mls_state(id, b"opaque mls state", 1)
            .expect("save");
        assert_eq!(
            store.load_mls_state(id).expect("load"),
            Some(b"opaque mls state".to_vec())
        );
    }

    #[test]
    fn direct_history_is_idempotent_and_ordered() {
        let store = Store::in_memory(&DatabaseKey::from_bytes([4; 32])).expect("store");
        let peer = IdentityId::from_bytes([5; 32]);
        let later = DirectMessageRecord {
            message_id: [2; 32],
            peer_identity: peer,
            sender_name: "Alice".into(),
            body: "later".into(),
            sent_at_unix: 20,
            outgoing: false,
        };
        let earlier = DirectMessageRecord {
            message_id: [1; 32],
            peer_identity: peer,
            sender_name: "You".into(),
            body: "earlier".into(),
            sent_at_unix: 10,
            outgoing: true,
        };
        assert!(store.save_direct_message(&later).expect("later"));
        assert!(store.save_direct_message(&earlier).expect("earlier"));
        assert!(!store.save_direct_message(&earlier).expect("duplicate"));
        assert_eq!(
            store.load_direct_messages(peer, 20).expect("history"),
            vec![earlier, later]
        );
    }
}
