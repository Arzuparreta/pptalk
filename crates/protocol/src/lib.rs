//! Public, versioned wire model used by pptalk peers and optional nodes.

mod codec;
mod event;
mod id;
mod invite;
mod media;
mod node;
mod sync;

pub use codec::{CodecError, WireDecode, WireEncode};
pub use event::{
    BlobManifest, CallEndReason, CausalFrontier, ConversationEvent, EventBody, EventKind,
    MessageContent, QualityMode, QualityProfile, ReceiptKind, TransportEnvelope,
};
pub use id::{CallId, ConversationId, DeviceId, EventId, IdentityId, NodeId, OpaqueId};
pub use invite::{ContactInvite, InviteError, ReachabilityRecord};
pub use media::{
    CallSignal, IceCandidate, MediaDatagram, MediaKind, MediaSignal, SessionDescription,
};
pub use node::{NodeCapability, NodeDescriptor, NodeService, ScopedCapability};
pub use sync::{ConversationFrontier, SyncErrorCode, SyncFrame};

/// Current incompatible wire format generation.
pub const PROTOCOL_VERSION: u16 = 1;

/// QUIC ALPN used for durable conversation traffic.
pub const SYNC_ALPN: &[u8] = b"pptalk/sync/1";

/// QUIC ALPN used for short-lived call control traffic.
pub const CALL_ALPN: &[u8] = b"pptalk/call/1";

/// QUIC datagram channel used for latency-sensitive encoded media packets.
pub const MEDIA_ALPN: &[u8] = b"pptalk/media/1";

/// Maximum accepted wire envelope before allocating payload buffers.
pub const MAX_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
