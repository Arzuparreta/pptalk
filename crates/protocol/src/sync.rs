use serde::{Deserialize, Serialize};

use crate::{
    CausalFrontier, ConversationEvent, ConversationId, DeviceId, EventId, PROTOCOL_VERSION,
};

/// Durable, idempotent conversation synchronization frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncFrame {
    Hello {
        version: u16,
        device: DeviceId,
        conversations: Vec<ConversationFrontier>,
    },
    Events {
        conversation_id: ConversationId,
        events: Vec<ConversationEvent>,
    },
    Ack {
        conversation_id: ConversationId,
        frontier: CausalFrontier,
    },
    FetchBlob {
        ciphertext_hash: [u8; 32],
        missing_chunks: Vec<u32>,
    },
    BlobChunk {
        ciphertext_hash: [u8; 32],
        chunk_index: u32,
        bytes: Vec<u8>,
    },
    EventAck {
        event_ids: Vec<EventId>,
    },
    Error {
        code: SyncErrorCode,
        detail: String,
    },
}

impl SyncFrame {
    pub fn hello(device: DeviceId, conversations: Vec<ConversationFrontier>) -> Self {
        Self::Hello {
            version: PROTOCOL_VERSION,
            device,
            conversations,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationFrontier {
    pub conversation_id: ConversationId,
    pub frontier: CausalFrontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncErrorCode {
    UnsupportedVersion,
    InvalidEnvelope,
    Unauthorized,
    MissingHistory,
    RateLimited,
}
