use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use pptalk_protocol::{DeviceId, IdentityId, WireEncode};
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone)]
pub struct DeviceKeyPair {
    signing: SigningKey,
}

impl std::fmt::Debug for DeviceKeyPair {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceKeyPair")
            .field("device_id", &self.device_id())
            .finish_non_exhaustive()
    }
}

impl DeviceKeyPair {
    pub fn generate(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        Self {
            signing: SigningKey::generate(rng),
        }
    }

    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(bytes),
        }
    }

    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::from_bytes(*blake3::hash(&self.public_key()).as_bytes())
    }

    pub fn sign_message(&self, bytes: &[u8]) -> Vec<u8> {
        self.signing.sign(bytes).to_bytes().to_vec()
    }

    pub fn verify_message(
        public_key: &[u8; 32],
        bytes: &[u8],
        signature: &[u8],
    ) -> Result<(), IdentityError> {
        let key =
            VerifyingKey::from_bytes(public_key).map_err(|_| IdentityError::InvalidPublicKey)?;
        let signature =
            Signature::from_slice(signature).map_err(|_| IdentityError::InvalidSignature)?;
        key.verify(bytes, &signature)
            .map_err(|_| IdentityError::InvalidSignature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: DeviceId,
    pub public_key: [u8; 32],
    pub label: String,
    pub added_at_unix: i64,
    pub revoked_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityEventKind {
    Genesis { public_key: [u8; 32], label: String },
    AddDevice { public_key: [u8; 32], label: String },
    RevokeDevice { device_id: DeviceId, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEvent {
    pub sequence: u64,
    pub previous_hash: [u8; 32],
    pub author_device: DeviceId,
    pub created_at_unix: i64,
    pub kind: IdentityEventKind,
    pub signature: Vec<u8>,
}

#[derive(Serialize)]
struct SignableIdentityEvent<'a> {
    sequence: u64,
    previous_hash: [u8; 32],
    author_device: DeviceId,
    created_at_unix: i64,
    kind: &'a IdentityEventKind,
}

impl IdentityEvent {
    fn signable_bytes(&self) -> Result<Vec<u8>, IdentityError> {
        SignableIdentityEvent {
            sequence: self.sequence,
            previous_hash: self.previous_hash,
            author_device: self.author_device,
            created_at_unix: self.created_at_unix,
            kind: &self.kind,
        }
        .to_wire()
        .map_err(IdentityError::Codec)
    }

    pub fn hash(&self) -> Result<[u8; 32], IdentityError> {
        Ok(*blake3::hash(&self.to_wire().map_err(IdentityError::Codec)?).as_bytes())
    }
}

#[derive(Debug, Clone)]
pub struct IdentityLog {
    identity_id: IdentityId,
    events: Vec<IdentityEvent>,
    devices: BTreeMap<DeviceId, DeviceRecord>,
}

impl IdentityLog {
    pub fn create(
        key: &DeviceKeyPair,
        label: impl Into<String>,
        created_at_unix: i64,
    ) -> Result<Self, IdentityError> {
        let kind = IdentityEventKind::Genesis {
            public_key: key.public_key(),
            label: label.into(),
        };
        let mut event = IdentityEvent {
            sequence: 0,
            previous_hash: [0; 32],
            author_device: key.device_id(),
            created_at_unix,
            kind,
            signature: Vec::new(),
        };
        event.signature = key.sign_message(&event.signable_bytes()?);
        let identity_id =
            IdentityId::from_bytes(*blake3::hash(&event.signable_bytes()?).as_bytes());
        Self::from_events(identity_id, vec![event])
    }

    pub fn from_events(
        identity_id: IdentityId,
        events: Vec<IdentityEvent>,
    ) -> Result<Self, IdentityError> {
        let mut log = Self {
            identity_id,
            events: Vec::new(),
            devices: BTreeMap::new(),
        };
        for event in events {
            log.append_verified(event)?;
        }
        Ok(log)
    }

    pub const fn identity_id(&self) -> IdentityId {
        self.identity_id
    }

    pub fn events(&self) -> &[IdentityEvent] {
        &self.events
    }

    pub fn devices(&self) -> impl Iterator<Item = &DeviceRecord> {
        self.devices.values()
    }

    pub fn active_device(&self, id: DeviceId) -> Option<&DeviceRecord> {
        self.devices
            .get(&id)
            .filter(|device| device.revoked_at_unix.is_none())
    }

    pub fn add_device(
        &mut self,
        author: &DeviceKeyPair,
        new_device: &DeviceKeyPair,
        label: impl Into<String>,
        created_at_unix: i64,
    ) -> Result<IdentityEvent, IdentityError> {
        self.append_signed(
            author,
            IdentityEventKind::AddDevice {
                public_key: new_device.public_key(),
                label: label.into(),
            },
            created_at_unix,
        )
    }

    pub fn revoke_device(
        &mut self,
        author: &DeviceKeyPair,
        device_id: DeviceId,
        reason: impl Into<String>,
        created_at_unix: i64,
    ) -> Result<IdentityEvent, IdentityError> {
        self.append_signed(
            author,
            IdentityEventKind::RevokeDevice {
                device_id,
                reason: reason.into(),
            },
            created_at_unix,
        )
    }

    fn append_signed(
        &mut self,
        author: &DeviceKeyPair,
        kind: IdentityEventKind,
        created_at_unix: i64,
    ) -> Result<IdentityEvent, IdentityError> {
        if self.active_device(author.device_id()).is_none() {
            return Err(IdentityError::UnauthorizedDevice(author.device_id()));
        }
        let mut event = IdentityEvent {
            sequence: self.events.len() as u64,
            previous_hash: self
                .events
                .last()
                .map(IdentityEvent::hash)
                .transpose()?
                .unwrap_or([0; 32]),
            author_device: author.device_id(),
            created_at_unix,
            kind,
            signature: Vec::new(),
        };
        event.signature = author.sign_message(&event.signable_bytes()?);
        self.append_verified(event.clone())?;
        Ok(event)
    }

    pub fn append_verified(&mut self, event: IdentityEvent) -> Result<(), IdentityError> {
        let expected_sequence = self.events.len() as u64;
        if event.sequence != expected_sequence {
            return Err(IdentityError::Sequence {
                expected: expected_sequence,
                actual: event.sequence,
            });
        }
        let expected_previous = self
            .events
            .last()
            .map(IdentityEvent::hash)
            .transpose()?
            .unwrap_or([0; 32]);
        if event.previous_hash != expected_previous {
            return Err(IdentityError::Fork);
        }

        let verifying_key = if event.sequence == 0 {
            match &event.kind {
                IdentityEventKind::Genesis { public_key, .. } => {
                    if event.author_device != device_id_from_public_key(public_key) {
                        return Err(IdentityError::DeviceIdMismatch);
                    }
                    VerifyingKey::from_bytes(public_key)
                        .map_err(|_| IdentityError::InvalidPublicKey)?
                }
                _ => return Err(IdentityError::MissingGenesis),
            }
        } else {
            let author = self
                .active_device(event.author_device)
                .ok_or(IdentityError::UnauthorizedDevice(event.author_device))?;
            VerifyingKey::from_bytes(&author.public_key)
                .map_err(|_| IdentityError::InvalidPublicKey)?
        };
        let signature =
            Signature::from_slice(&event.signature).map_err(|_| IdentityError::InvalidSignature)?;
        verifying_key
            .verify(&event.signable_bytes()?, &signature)
            .map_err(|_| IdentityError::InvalidSignature)?;

        match &event.kind {
            IdentityEventKind::Genesis { public_key, label }
            | IdentityEventKind::AddDevice { public_key, label } => {
                let device_id = device_id_from_public_key(public_key);
                if self.devices.contains_key(&device_id) {
                    return Err(IdentityError::DuplicateDevice(device_id));
                }
                self.devices.insert(
                    device_id,
                    DeviceRecord {
                        device_id,
                        public_key: *public_key,
                        label: label.clone(),
                        added_at_unix: event.created_at_unix,
                        revoked_at_unix: None,
                    },
                );
            }
            IdentityEventKind::RevokeDevice { device_id, .. } => {
                let device = self
                    .devices
                    .get_mut(device_id)
                    .ok_or(IdentityError::UnknownDevice(*device_id))?;
                if device.revoked_at_unix.is_some() {
                    return Err(IdentityError::AlreadyRevoked(*device_id));
                }
                device.revoked_at_unix = Some(event.created_at_unix);
            }
        }
        self.events.push(event);
        Ok(())
    }
}

fn device_id_from_public_key(public_key: &[u8; 32]) -> DeviceId {
    DeviceId::from_bytes(*blake3::hash(public_key).as_bytes())
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("identity log must begin with a genesis event")]
    MissingGenesis,
    #[error("identity event sequence mismatch: expected {expected}, got {actual}")]
    Sequence { expected: u64, actual: u64 },
    #[error("identity log fork detected")]
    Fork,
    #[error("identity event has an invalid signature")]
    InvalidSignature,
    #[error("identity event contains an invalid public key")]
    InvalidPublicKey,
    #[error("device id does not match its public key")]
    DeviceIdMismatch,
    #[error("device {0} is not currently authorized")]
    UnauthorizedDevice(DeviceId),
    #[error("device {0} already exists")]
    DuplicateDevice(DeviceId),
    #[error("device {0} does not exist")]
    UnknownDevice(DeviceId),
    #[error("device {0} is already revoked")]
    AlreadyRevoked(DeviceId),
    #[error("could not encode identity event: {0}")]
    Codec(#[source] pptalk_protocol::CodecError),
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;

    use super::*;

    #[test]
    fn links_and_revokes_independent_devices() {
        let first = DeviceKeyPair::generate(&mut OsRng);
        let second = DeviceKeyPair::generate(&mut OsRng);
        let mut log = IdentityLog::create(&first, "desktop", 1).expect("create");
        log.add_device(&first, &second, "laptop", 2).expect("link");
        assert!(log.active_device(second.device_id()).is_some());
        log.revoke_device(&second, first.device_id(), "retired", 3)
            .expect("revoke");
        assert!(log.active_device(first.device_id()).is_none());
        assert_eq!(log.devices().count(), 2);
    }

    #[test]
    fn detects_a_forked_previous_hash() {
        let key = DeviceKeyPair::generate(&mut OsRng);
        let log = IdentityLog::create(&key, "desktop", 1).expect("create");
        let mut event = log.events()[0].clone();
        event.sequence = 1;
        event.previous_hash = [9; 32];
        let mut reloaded = log.clone();
        assert!(matches!(
            reloaded.append_verified(event),
            Err(IdentityError::Fork)
        ));
    }
}
