use serde::{Deserialize, Serialize};

use crate::{CallId, DeviceId, IdentityId, PROTOCOL_VERSION, QualityProfile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaDatagram {
    pub version: u16,
    pub call_id: CallId,
    pub sender: DeviceId,
    pub kind: MediaKind,
    pub sequence: u64,
    pub timestamp_micros: u64,
    pub marker: bool,
    pub payload: Vec<u8>,
}

impl MediaDatagram {
    pub fn new(
        call_id: CallId,
        sender: DeviceId,
        kind: MediaKind,
        sequence: u64,
        timestamp_micros: u64,
        marker: bool,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            call_id,
            sender,
            kind,
            sequence,
            timestamp_micros,
            marker,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Voice,
    Camera,
    Screen,
    SystemAudio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDescription {
    pub kind: String,
    pub sdp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceCandidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_m_line_index: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaSignal {
    Description(SessionDescription),
    Ice(IceCandidate),
    Publish {
        kind: MediaKind,
        profile: QualityProfile,
    },
    Unpublish {
        kind: MediaKind,
    },
    Subscribe {
        publisher: DeviceId,
        kind: MediaKind,
        enabled: bool,
    },
    RouterOffer {
        endpoint: String,
        token: Vec<u8>,
    },
    RouterReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallSignal {
    Invite {
        call_id: CallId,
        selected: Vec<IdentityId>,
        ring: bool,
    },
    Join {
        call_id: CallId,
    },
    Reject {
        call_id: CallId,
        #[serde(default)]
        missed: bool,
    },
    Hold {
        call_id: CallId,
    },
    Resume {
        call_id: CallId,
    },
    Leave {
        call_id: CallId,
    },
    Media {
        call_id: CallId,
        signal: MediaSignal,
    },
    Ping {
        call_id: CallId,
        nonce: u64,
    },
    Pong {
        call_id: CallId,
        nonce: u64,
    },
}
