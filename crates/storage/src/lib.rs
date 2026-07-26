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

const SCHEMA_VERSION: i64 = 2;

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

             UPDATE schema_meta SET version = 1 WHERE singleton = 1 AND version = 0;
             COMMIT;",
        )?;
        let version: i64 = self.connection.query_row(
            "SELECT version FROM schema_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        if version == 1 {
            self.connection.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE direct_messages ADD COLUMN reply_to BLOB;
                 ALTER TABLE direct_messages ADD COLUMN edited INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE direct_messages ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE direct_messages ADD COLUMN delivery TEXT NOT NULL DEFAULT 'delivered';
                 ALTER TABLE direct_messages ADD COLUMN file_path TEXT;

                 CREATE TABLE conversation_settings (
                   conversation_key TEXT PRIMARY KEY,
                   pinned INTEGER NOT NULL DEFAULT 0,
                   archived INTEGER NOT NULL DEFAULT 0,
                   muted_until_unix INTEGER,
                   unread_count INTEGER NOT NULL DEFAULT 0,
                   last_summary TEXT NOT NULL DEFAULT '',
                   last_activity_unix INTEGER NOT NULL DEFAULT 0,
                   notification_preview INTEGER NOT NULL DEFAULT 1
                 ) WITHOUT ROWID;

                 CREATE TABLE call_events (
                   call_id BLOB PRIMARY KEY,
                   conversation_key TEXT NOT NULL,
                   direction TEXT NOT NULL,
                   outcome TEXT NOT NULL,
                   started_at_unix INTEGER NOT NULL,
                   duration_ms INTEGER NOT NULL DEFAULT 0
                 ) WITHOUT ROWID;
                 CREATE INDEX call_events_conversation
                   ON call_events(conversation_key, started_at_unix DESC);

                 CREATE VIRTUAL TABLE direct_messages_fts USING fts5(
                   message_id UNINDEXED, body, sender_name
                 );
                 INSERT INTO direct_messages_fts(message_id, body, sender_name)
                   SELECT message_id, body, sender_name FROM direct_messages;
                 CREATE TRIGGER direct_messages_fts_insert AFTER INSERT ON direct_messages BEGIN
                   INSERT INTO direct_messages_fts(message_id, body, sender_name)
                   VALUES (new.message_id, new.body, new.sender_name);
                 END;
                 CREATE TRIGGER direct_messages_fts_delete AFTER DELETE ON direct_messages BEGIN
                   DELETE FROM direct_messages_fts WHERE message_id = old.message_id;
                 END;
                 CREATE TRIGGER direct_messages_fts_update AFTER UPDATE OF body, sender_name ON direct_messages BEGIN
                   DELETE FROM direct_messages_fts WHERE message_id = old.message_id;
                   INSERT INTO direct_messages_fts(message_id, body, sender_name)
                   VALUES (new.message_id, new.body, new.sender_name);
                 END;

                 UPDATE schema_meta SET version = 2 WHERE singleton = 1;
                 COMMIT;",
            )?;
        }
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
               message_id, peer_identity, sender_name, body, sent_at_unix, outgoing,
               reply_to, edited, deleted, delivery, file_path
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                message.message_id.as_slice(),
                message.peer_identity.as_bytes().as_slice(),
                message.sender_name,
                message.body,
                message.sent_at_unix,
                message.outgoing,
                message.reply_to.map(|value| value.to_vec()),
                message.edited,
                message.deleted,
                message.delivery,
                message.file_path,
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
            "SELECT message_id, peer_identity, sender_name, body, sent_at_unix, outgoing,
                    reply_to, edited, deleted, delivery, file_path
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
                    reply_to: row
                        .get::<_, Option<Vec<u8>>>(6)?
                        .map(|value| bytes_32(&value)),
                    edited: row.get(7)?,
                    deleted: row.get(8)?,
                    delivery: row.get(9)?,
                    file_path: row.get(10)?,
                })
            })?;
        let mut messages = rows.collect::<Result<Vec<_>, _>>()?;
        messages.reverse();
        Ok(messages)
    }

    pub fn load_all_direct_messages(&self) -> Result<Vec<DirectMessageRecord>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT message_id, peer_identity, sender_name, body, sent_at_unix, outgoing,
                    reply_to, edited, deleted, delivery, file_path
             FROM direct_messages ORDER BY sent_at_unix, message_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DirectMessageRecord {
                message_id: bytes_32(row.get_ref(0)?.as_blob()?),
                peer_identity: identity_id_from_blob(row.get_ref(1)?.as_blob()?),
                sender_name: row.get(2)?,
                body: row.get(3)?,
                sent_at_unix: row.get(4)?,
                outgoing: row.get(5)?,
                reply_to: row
                    .get::<_, Option<Vec<u8>>>(6)?
                    .map(|value| bytes_32(&value)),
                edited: row.get(7)?,
                deleted: row.get(8)?,
                delivery: row.get(9)?,
                file_path: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    pub fn update_direct_message(
        &self,
        message_id: [u8; 32],
        peer_identity: IdentityId,
        outgoing: bool,
        body: Option<&str>,
        deleted: bool,
    ) -> Result<bool, StorageError> {
        let changed = if deleted {
            self.connection.execute(
                "UPDATE direct_messages SET body = '', deleted = 1, edited = 0
                 WHERE message_id = ?1 AND peer_identity = ?2 AND outgoing = ?3",
                params![
                    message_id.as_slice(),
                    peer_identity.as_bytes().as_slice(),
                    outgoing
                ],
            )?
        } else if let Some(body) = body {
            self.connection.execute(
                "UPDATE direct_messages SET body = ?2, edited = 1
                 WHERE message_id = ?1 AND peer_identity = ?3 AND outgoing = ?4 AND deleted = 0",
                params![
                    message_id.as_slice(),
                    body,
                    peer_identity.as_bytes().as_slice(),
                    outgoing
                ],
            )?
        } else {
            0
        };
        Ok(changed == 1)
    }

    pub fn set_direct_delivery(
        &self,
        message_id: [u8; 32],
        delivery: &str,
    ) -> Result<bool, StorageError> {
        let changed = self.connection.execute(
            "UPDATE direct_messages SET delivery = ?2 WHERE message_id = ?1",
            params![message_id.as_slice(), delivery],
        )?;
        Ok(changed == 1)
    }

    pub fn delete_direct_message_local(
        &self,
        message_id: [u8; 32],
        peer_identity: IdentityId,
    ) -> Result<bool, StorageError> {
        let changed = self.connection.execute(
            "UPDATE direct_messages SET body = '', deleted = 1, edited = 0
             WHERE message_id = ?1 AND peer_identity = ?2",
            params![message_id.as_slice(), peer_identity.as_bytes().as_slice()],
        )?;
        Ok(changed == 1)
    }

    pub fn search_direct_messages(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<DirectMessageRecord>, StorageError> {
        let limit = i64::try_from(limit).map_err(|_| StorageError::IntegerOverflow)?;
        let mut statement = self.connection.prepare(
            "SELECT d.message_id, d.peer_identity, d.sender_name, d.body, d.sent_at_unix,
                    d.outgoing, d.reply_to, d.edited, d.deleted, d.delivery, d.file_path
             FROM direct_messages_fts f
             JOIN direct_messages d ON d.message_id = f.message_id
             WHERE direct_messages_fts MATCH ?1 AND d.deleted = 0
             ORDER BY rank LIMIT ?2",
        )?;
        let rows = statement.query_map(params![query, limit], |row| {
            Ok(DirectMessageRecord {
                message_id: bytes_32(row.get_ref(0)?.as_blob()?),
                peer_identity: identity_id_from_blob(row.get_ref(1)?.as_blob()?),
                sender_name: row.get(2)?,
                body: row.get(3)?,
                sent_at_unix: row.get(4)?,
                outgoing: row.get(5)?,
                reply_to: row
                    .get::<_, Option<Vec<u8>>>(6)?
                    .map(|value| bytes_32(&value)),
                edited: row.get(7)?,
                deleted: row.get(8)?,
                delivery: row.get(9)?,
                file_path: row.get(10)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    pub fn save_conversation_settings(
        &self,
        settings: &ConversationSettings,
    ) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO conversation_settings(
               conversation_key, pinned, archived, muted_until_unix, unread_count,
               last_summary, last_activity_unix, notification_preview
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(conversation_key) DO UPDATE SET
               pinned=excluded.pinned, archived=excluded.archived,
               muted_until_unix=excluded.muted_until_unix, unread_count=excluded.unread_count,
               last_summary=excluded.last_summary, last_activity_unix=excluded.last_activity_unix,
               notification_preview=excluded.notification_preview",
            params![
                settings.conversation_key,
                settings.pinned,
                settings.archived,
                settings.muted_until_unix,
                settings.unread_count,
                settings.last_summary,
                settings.last_activity_unix,
                settings.notification_preview,
            ],
        )?;
        Ok(())
    }

    pub fn load_conversation_settings(&self) -> Result<Vec<ConversationSettings>, StorageError> {
        let mut statement = self.connection.prepare(
            "SELECT conversation_key, pinned, archived, muted_until_unix, unread_count,
                    last_summary, last_activity_unix, notification_preview
             FROM conversation_settings ORDER BY pinned DESC, last_activity_unix DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ConversationSettings {
                conversation_key: row.get(0)?,
                pinned: row.get(1)?,
                archived: row.get(2)?,
                muted_until_unix: row.get(3)?,
                unread_count: row.get(4)?,
                last_summary: row.get(5)?,
                last_activity_unix: row.get(6)?,
                notification_preview: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    pub fn save_call_event(&self, event: &CallEventRecord) -> Result<(), StorageError> {
        self.connection.execute(
            "INSERT INTO call_events(call_id, conversation_key, direction, outcome,
               started_at_unix, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(call_id) DO UPDATE SET outcome=excluded.outcome,
               duration_ms=excluded.duration_ms",
            params![
                event.call_id.as_slice(),
                event.conversation_key,
                event.direction,
                event.outcome,
                event.started_at_unix,
                i64::try_from(event.duration_ms).map_err(|_| StorageError::IntegerOverflow)?,
            ],
        )?;
        Ok(())
    }

    pub fn load_call_events(
        &self,
        conversation_key: &str,
        limit: usize,
    ) -> Result<Vec<CallEventRecord>, StorageError> {
        let limit = i64::try_from(limit).map_err(|_| StorageError::IntegerOverflow)?;
        let mut statement = self.connection.prepare(
            "SELECT call_id, conversation_key, direction, outcome, started_at_unix, duration_ms
             FROM call_events WHERE conversation_key = ?1
             ORDER BY started_at_unix DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![conversation_key, limit], |row| {
            Ok(CallEventRecord {
                call_id: bytes_32(row.get_ref(0)?.as_blob()?),
                conversation_key: row.get(1)?,
                direction: row.get(2)?,
                outcome: row.get(3)?,
                started_at_unix: row.get(4)?,
                duration_ms: u64::try_from(row.get::<_, i64>(5)?).unwrap_or_default(),
            })
        })?;
        let mut events = rows.collect::<Result<Vec<_>, _>>()?;
        events.reverse();
        Ok(events)
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
    pub reply_to: Option<[u8; 32]>,
    pub edited: bool,
    pub deleted: bool,
    pub delivery: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSettings {
    pub conversation_key: String,
    pub pinned: bool,
    pub archived: bool,
    pub muted_until_unix: Option<i64>,
    pub unread_count: u32,
    pub last_summary: String,
    pub last_activity_unix: i64,
    pub notification_preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEventRecord {
    pub call_id: [u8; 32],
    pub conversation_key: String,
    pub direction: String,
    pub outcome: String,
    pub started_at_unix: i64,
    pub duration_ms: u64,
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
            reply_to: None,
            edited: false,
            deleted: false,
            delivery: "delivered".into(),
            file_path: None,
        };
        let earlier = DirectMessageRecord {
            message_id: [1; 32],
            peer_identity: peer,
            sender_name: "You".into(),
            body: "earlier".into(),
            sent_at_unix: 10,
            outgoing: true,
            reply_to: None,
            edited: false,
            deleted: false,
            delivery: "pending".into(),
            file_path: None,
        };
        assert!(store.save_direct_message(&later).expect("later"));
        assert!(store.save_direct_message(&earlier).expect("earlier"));
        assert!(!store.save_direct_message(&earlier).expect("duplicate"));
        assert_eq!(
            store.load_direct_messages(peer, 20).expect("history"),
            vec![earlier.clone(), later.clone()]
        );
        assert_eq!(
            store.search_direct_messages("earlier", 10).expect("search"),
            vec![earlier.clone()]
        );
        assert!(
            store
                .update_direct_message(earlier.message_id, peer, true, Some("edited text"), false)
                .expect("edit")
        );
        let edited = store
            .load_direct_messages(peer, 20)
            .expect("edited history");
        assert_eq!(edited[0].body, "edited text");
        assert!(edited[0].edited);
        assert!(
            store
                .set_direct_delivery(earlier.message_id, "delivered")
                .expect("delivery")
        );
        assert!(
            store
                .delete_direct_message_local(later.message_id, peer)
                .expect("local delete")
        );
        assert!(
            store
                .update_direct_message(earlier.message_id, peer, true, None, true)
                .expect("delete")
        );
        assert!(store.load_direct_messages(peer, 20).expect("deleted")[0].deleted);
    }

    #[test]
    fn persists_conversation_settings_and_call_events() {
        let store = Store::in_memory(&DatabaseKey::from_bytes([6; 32])).expect("store");
        let settings = ConversationSettings {
            conversation_key: "direct:alice".into(),
            pinned: true,
            archived: false,
            muted_until_unix: Some(100),
            unread_count: 3,
            last_summary: "hola".into(),
            last_activity_unix: 99,
            notification_preview: false,
        };
        store
            .save_conversation_settings(&settings)
            .expect("save settings");
        assert_eq!(
            store.load_conversation_settings().expect("settings"),
            vec![settings]
        );
        store
            .save_call_event(&CallEventRecord {
                call_id: [7; 32],
                conversation_key: "direct:alice".into(),
                direction: "outgoing".into(),
                outcome: "missed".into(),
                started_at_unix: 10,
                duration_ms: 0,
            })
            .expect("call event");
        assert_eq!(
            store.load_call_events("direct:alice", 10).expect("calls")[0].outcome,
            "missed"
        );
    }

    #[test]
    fn migrates_a_v1_profile_without_losing_direct_history() {
        let directory = std::env::temp_dir().join(format!(
            "pptalk-storage-migration-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).expect("temporary directory");
        let database = directory.join("profile.db");
        let key = DatabaseKey::from_bytes([8; 32]);
        {
            let connection = Connection::open(&database).expect("legacy database");
            connection
                .execute_batch(&format!(
                    "PRAGMA key = \"x'{}'\";
                     CREATE TABLE schema_meta (
                       singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                       version INTEGER NOT NULL
                     );
                     INSERT INTO schema_meta(singleton, version) VALUES (1, 1);
                     CREATE TABLE direct_messages (
                       message_id BLOB PRIMARY KEY,
                       peer_identity BLOB NOT NULL,
                       sender_name TEXT NOT NULL,
                       body TEXT NOT NULL,
                       sent_at_unix INTEGER NOT NULL,
                       outgoing INTEGER NOT NULL
                     ) WITHOUT ROWID;",
                    hex::encode(key.expose_for_profile())
                ))
                .expect("legacy schema");
            connection
                .execute(
                    "INSERT INTO direct_messages VALUES (?1, ?2, 'Alice', 'before migration', 10, 0)",
                    params![[1_u8; 32].as_slice(), [2_u8; 32].as_slice()],
                )
                .expect("legacy message");
        }

        {
            let store = Store::open(&database, &key).expect("migrated store");
            let messages = store
                .load_direct_messages(IdentityId::from_bytes([2; 32]), 10)
                .expect("migrated history");
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].body, "before migration");
            assert!(!messages[0].edited);
            assert!(!messages[0].deleted);
            assert_eq!(messages[0].delivery, "delivered");
            assert_eq!(
                store
                    .search_direct_messages("migration", 10)
                    .expect("migrated search")
                    .len(),
                1
            );
        }
        std::fs::remove_dir_all(directory).expect("remove temporary directory");
    }
}
