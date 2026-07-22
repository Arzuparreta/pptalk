use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::MAX_ENVELOPE_BYTES;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("wire value exceeds {MAX_ENVELOPE_BYTES} bytes")]
    TooLarge,
    #[error("could not encode wire value: {0}")]
    Encode(#[source] ciborium::ser::Error<std::io::Error>),
    #[error("could not decode wire value: {0}")]
    Decode(#[source] ciborium::de::Error<std::io::Error>),
}

pub trait WireEncode: Serialize {
    fn to_wire(&self) -> Result<Vec<u8>, CodecError> {
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(self, &mut bytes).map_err(CodecError::Encode)?;
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(CodecError::TooLarge);
        }
        Ok(bytes)
    }
}

impl<T: Serialize> WireEncode for T {}

pub trait WireDecode: DeserializeOwned {
    fn from_wire(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(CodecError::TooLarge);
        }
        ciborium::de::from_reader(bytes).map_err(CodecError::Decode)
    }
}

impl<T: DeserializeOwned> WireDecode for T {}
