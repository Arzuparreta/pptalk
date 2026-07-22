use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

use crate::{DeviceId, IdentityId, PROTOCOL_VERSION, WireDecode, WireEncode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityRecord {
    pub version: u16,
    pub device_id: DeviceId,
    pub expires_unix: i64,
    pub endpoint_id: String,
    pub direct_candidates: Vec<String>,
    pub relay_candidates: Vec<Url>,
    pub mailbox_candidates: Vec<Url>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactInvite {
    pub version: u16,
    pub inviter_identity: IdentityId,
    pub inviter_device: DeviceId,
    pub inviter_device_public_key: [u8; 32],
    pub display_name: String,
    pub expires_unix: i64,
    pub one_time_secret: [u8; 32],
    pub reachability: ReachabilityRecord,
    pub signature: Vec<u8>,
}

impl ContactInvite {
    pub fn to_url(&self) -> Result<Url, InviteError> {
        let payload = URL_SAFE_NO_PAD.encode(self.to_wire()?);
        Url::parse(&format!("pptalk://contact/v1#{payload}")).map_err(InviteError::Url)
    }

    pub fn from_url(url: &Url, now: OffsetDateTime) -> Result<Self, InviteError> {
        if url.scheme() != "pptalk" || url.host_str() != Some("contact") || url.path() != "/v1" {
            return Err(InviteError::Unsupported);
        }
        let fragment = url.fragment().ok_or(InviteError::MissingPayload)?;
        let bytes = URL_SAFE_NO_PAD
            .decode(fragment)
            .map_err(InviteError::Base64)?;
        let invite = Self::from_wire(&bytes)?;
        if invite.version != PROTOCOL_VERSION {
            return Err(InviteError::Unsupported);
        }
        if invite.expires_unix <= now.unix_timestamp() {
            return Err(InviteError::Expired);
        }
        Ok(invite)
    }
}

#[derive(Debug, Error)]
pub enum InviteError {
    #[error("invite URL has an unsupported scheme or version")]
    Unsupported,
    #[error("invite URL has no payload")]
    MissingPayload,
    #[error("invite has expired")]
    Expired,
    #[error("invite payload is not valid base64: {0}")]
    Base64(#[source] base64::DecodeError),
    #[error("invite payload is not valid wire data: {0}")]
    Codec(#[from] crate::CodecError),
    #[error("could not construct invite URL: {0}")]
    Url(#[source] url::ParseError),
}
