use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ConversationId, DeviceId, EventId, IdentityId, PROTOCOL_VERSION};

pub type CausalFrontier = BTreeMap<DeviceId, u64>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportEnvelope {
    pub version: u16,
    pub routing_capability: [u8; 32],
    pub ciphertext_hash: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub padding: Vec<u8>,
}

impl TransportEnvelope {
    pub fn new(routing_capability: [u8; 32], ciphertext: Vec<u8>, pad_to: usize) -> Self {
        let ciphertext_hash = *blake3::hash(&ciphertext).as_bytes();
        let padding_len = pad_to.saturating_sub(ciphertext.len());
        Self {
            version: PROTOCOL_VERSION,
            routing_capability,
            ciphertext_hash,
            ciphertext,
            padding: vec![0; padding_len],
        }
    }

    pub fn verify(&self) -> bool {
        self.version == PROTOCOL_VERSION
            && blake3::hash(&self.ciphertext).as_bytes() == &self.ciphertext_hash
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationEvent {
    pub version: u16,
    pub conversation_id: ConversationId,
    pub event_id: EventId,
    pub author_identity: IdentityId,
    pub author_device: DeviceId,
    pub device_sequence: u64,
    pub causal_frontier: CausalFrontier,
    pub logical_time_ms: i64,
    pub body: EventBody,
}

impl ConversationEvent {
    pub fn kind(&self) -> EventKind {
        self.body.kind()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    MessageCreate,
    MessageEdit,
    MessageDelete,
    ReactionSet,
    Receipt,
    GroupRename,
    GroupAvatar,
    MembershipAdd,
    MembershipRemove,
    OwnershipTransfer,
    BlobManifest,
    CallEnded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventBody {
    MessageCreate {
        content: MessageContent,
    },
    MessageEdit {
        target: EventId,
        content: MessageContent,
    },
    MessageDelete {
        target: EventId,
    },
    ReactionSet {
        target: EventId,
        emoji: String,
        active: bool,
    },
    Receipt {
        target: EventId,
        kind: ReceiptKind,
    },
    GroupRename {
        name: String,
    },
    GroupAvatar {
        blob: Option<[u8; 32]>,
    },
    MembershipAdd {
        identity: IdentityId,
        welcome: Vec<u8>,
    },
    MembershipRemove {
        identity: IdentityId,
        commit: Vec<u8>,
    },
    OwnershipTransfer {
        from: IdentityId,
        to: IdentityId,
    },
    BlobManifest(BlobManifest),
    CallEnded {
        duration_ms: u64,
        reason: CallEndReason,
    },
}

impl EventBody {
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::MessageCreate { .. } => EventKind::MessageCreate,
            Self::MessageEdit { .. } => EventKind::MessageEdit,
            Self::MessageDelete { .. } => EventKind::MessageDelete,
            Self::ReactionSet { .. } => EventKind::ReactionSet,
            Self::Receipt { .. } => EventKind::Receipt,
            Self::GroupRename { .. } => EventKind::GroupRename,
            Self::GroupAvatar { .. } => EventKind::GroupAvatar,
            Self::MembershipAdd { .. } => EventKind::MembershipAdd,
            Self::MembershipRemove { .. } => EventKind::MembershipRemove,
            Self::OwnershipTransfer { .. } => EventKind::OwnershipTransfer,
            Self::BlobManifest(_) => EventKind::BlobManifest,
            Self::CallEnded { .. } => EventKind::CallEnded,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: String,
    pub reply_to: Option<EventId>,
    pub attachment_ids: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceiptKind {
    Delivered,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallEndReason {
    LastParticipantLeft,
    NetworkUnavailable,
    ReplacedByNewSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobManifest {
    pub ciphertext_hash: [u8; 32],
    pub byte_len: u64,
    pub chunk_size: u32,
    pub chunk_hashes: Vec<[u8; 32]>,
    pub media_type: String,
    pub file_name: String,
    pub key_envelope: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityMode {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityProfile {
    pub mode: QualityMode,
    pub width: u16,
    pub height: u16,
    pub frames_per_second: u8,
    pub bitrate_kbps: u32,
    pub codec: Option<String>,
}
