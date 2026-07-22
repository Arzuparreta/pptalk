use serde::{Deserialize, Serialize};
use url::Url;

use crate::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeService {
    Rendezvous,
    Relay,
    Turn,
    Mailbox,
    Sfu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapability {
    pub service: NodeService,
    pub max_bytes: Option<u64>,
    pub max_participants: Option<u16>,
    pub max_bitrate_kbps: Option<u32>,
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDescriptor {
    pub node_id: NodeId,
    pub endpoints: Vec<Url>,
    pub services: Vec<NodeCapability>,
    pub public_key: Vec<u8>,
    pub priority: i16,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedCapability {
    pub node_id: NodeId,
    pub service: NodeService,
    pub token: [u8; 32],
    pub expires_unix: i64,
    pub max_bytes: Option<u64>,
    pub max_bitrate_kbps: Option<u32>,
}
