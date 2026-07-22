use std::{fmt, str::FromStr};

use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A fixed-size, unguessable identifier. Semantic IDs are strong aliases below.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OpaqueId([u8; 32]);

impl OpaqueId {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn random(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        let mut bytes = [0_u8; 32];
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn short(&self) -> String {
        hex::encode(&self.0[..6])
    }
}

impl fmt::Debug for OpaqueId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("OpaqueId")
            .field(&self.short())
            .finish()
    }
}

impl fmt::Display for OpaqueId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl FromStr for OpaqueId {
    type Err = ParseIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(value).map_err(ParseIdError::Hex)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| ParseIdError::Length)?;
        Ok(Self(bytes))
    }
}

#[derive(Debug, Error)]
pub enum ParseIdError {
    #[error("identifier must contain 32 bytes")]
    Length,
    #[error("identifier is not valid hexadecimal: {0}")]
    Hex(#[from] hex::FromHexError),
}

macro_rules! semantic_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub OpaqueId);

        impl $name {
            pub fn random(rng: &mut (impl CryptoRng + RngCore)) -> Self {
                Self(OpaqueId::random(rng))
            }

            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(OpaqueId::from_bytes(bytes))
            }

            pub const fn as_bytes(&self) -> &[u8; 32] {
                self.0.as_bytes()
            }

            pub fn short(&self) -> String {
                self.0.short()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Self(value.parse()?))
            }
        }
    };
}

semantic_id!(IdentityId);
semantic_id!(DeviceId);
semantic_id!(ConversationId);
semantic_id!(EventId);
semantic_id!(NodeId);
semantic_id!(CallId);
