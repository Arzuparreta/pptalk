//! RFC 9420 state machine used by private group conversations.

use std::collections::BTreeMap;

use ::tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

/// Owns one device's MLS credential, key store and active group states.
pub struct MlsClient {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
    groups: BTreeMap<Vec<u8>, MlsGroup>,
}

impl std::fmt::Debug for MlsClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MlsClient")
            .field("groups", &self.groups.keys())
            .finish_non_exhaustive()
    }
}

impl MlsClient {
    pub fn new(identity: impl Into<Vec<u8>>) -> Result<Self, MlsError> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm()).map_err(operation)?;
        signer.store(provider.storage()).map_err(operation)?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(identity.into()).into(),
            signature_key: signer.public().into(),
        };
        Ok(Self {
            provider,
            signer,
            credential,
            groups: BTreeMap::new(),
        })
    }

    /// Produces a single-use asynchronous key package for a group owner.
    pub fn key_package(&self) -> Result<Vec<u8>, MlsError> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential.clone(),
            )
            .map_err(operation)?;
        bundle
            .key_package()
            .tls_serialize_detached()
            .map_err(operation)
    }

    pub fn create_group(&mut self, group_id: &[u8]) -> Result<(), MlsError> {
        if self.groups.contains_key(group_id) {
            return Err(MlsError::GroupExists);
        }
        let config = MlsGroupCreateConfig::builder()
            .use_ratchet_tree_extension(true)
            .build();
        let group = MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &config,
            GroupId::from_slice(group_id),
            self.credential.clone(),
        )
        .map_err(operation)?;
        self.groups.insert(group_id.to_vec(), group);
        Ok(())
    }

    /// Adds one device and advances the owner to the new epoch. The returned
    /// Welcome is enough for the new member because the ratchet tree extension is enabled.
    pub fn add_member(&mut self, group_id: &[u8], key_package: &[u8]) -> Result<Vec<u8>, MlsError> {
        self.add_members(group_id, std::slice::from_ref(&key_package))
    }

    pub fn add_members(
        &mut self,
        group_id: &[u8],
        key_packages: &[&[u8]],
    ) -> Result<Vec<u8>, MlsError> {
        let key_packages = key_packages
            .iter()
            .map(|bytes| {
                let mut bytes = *bytes;
                KeyPackageIn::tls_deserialize(&mut bytes)
                    .map_err(operation)?
                    .validate(self.provider.crypto(), ProtocolVersion::Mls10)
                    .map_err(operation)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or(MlsError::UnknownGroup)?;
        let (_, welcome, _) = group
            .add_members(&self.provider, &self.signer, &key_packages)
            .map_err(operation)?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(operation)?;
        welcome.tls_serialize_detached().map_err(operation)
    }

    pub fn add_member_with_commit(
        &mut self,
        group_id: &[u8],
        key_package: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), MlsError> {
        let mut bytes = key_package;
        let key_package = KeyPackageIn::tls_deserialize(&mut bytes)
            .map_err(operation)?
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(operation)?;
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or(MlsError::UnknownGroup)?;
        let (commit, welcome, _) = group
            .add_members(&self.provider, &self.signer, &[key_package])
            .map_err(operation)?;
        let commit = commit.tls_serialize_detached().map_err(operation)?;
        let welcome = welcome.tls_serialize_detached().map_err(operation)?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(operation)?;
        Ok((welcome, commit))
    }

    pub fn join_group(&mut self, group_id: &[u8], welcome: &[u8]) -> Result<(), MlsError> {
        if self.groups.contains_key(group_id) {
            return Err(MlsError::GroupExists);
        }
        let mut welcome_bytes = welcome;
        let MlsMessageBodyIn::Welcome(welcome) = MlsMessageIn::tls_deserialize(&mut welcome_bytes)
            .map_err(operation)?
            .extract()
        else {
            return Err(MlsError::ExpectedWelcome);
        };
        let group = StagedWelcome::new_from_welcome(
            &self.provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            None,
        )
        .map_err(operation)?
        .into_group(&self.provider)
        .map_err(operation)?;
        self.groups.insert(group_id.to_vec(), group);
        Ok(())
    }

    pub fn encrypt(&mut self, group_id: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, MlsError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or(MlsError::UnknownGroup)?;
        group
            .create_message(&self.provider, &self.signer, plaintext)
            .map_err(operation)?
            .tls_serialize_detached()
            .map_err(operation)
    }

    pub fn remove_member(
        &mut self,
        group_id: &[u8],
        credential_identity: &[u8],
    ) -> Result<Vec<u8>, MlsError> {
        self.remove_members(group_id, &[credential_identity])
    }

    pub fn remove_members(
        &mut self,
        group_id: &[u8],
        credential_identities: &[&[u8]],
    ) -> Result<Vec<u8>, MlsError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or(MlsError::UnknownGroup)?;
        let indices = group
            .members()
            .filter_map(|member| {
                BasicCredential::try_from(member.credential)
                    .ok()
                    .filter(|credential| {
                        credential_identities
                            .iter()
                            .any(|identity| credential.identity() == *identity)
                    })
                    .map(|_| member.index)
            })
            .collect::<Vec<_>>();
        if indices.len() != credential_identities.len() {
            return Err(MlsError::UnknownMember);
        }
        let (commit, _, _) = group
            .remove_members(&self.provider, &self.signer, &indices)
            .map_err(operation)?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(operation)?;
        commit.tls_serialize_detached().map_err(operation)
    }

    pub fn decrypt(&mut self, group_id: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, MlsError> {
        let mut ciphertext_bytes = ciphertext;
        let message = MlsMessageIn::tls_deserialize(&mut ciphertext_bytes)
            .map_err(operation)?
            .try_into_protocol_message()
            .map_err(operation)?;
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or(MlsError::UnknownGroup)?;
        let processed = group
            .process_message(&self.provider, message)
            .map_err(operation)?;
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(message) => Ok(message.into_bytes()),
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                group
                    .merge_staged_commit(&self.provider, *commit)
                    .map_err(operation)?;
                Err(MlsError::ControlMessage)
            }
            _ => Err(MlsError::ControlMessage),
        }
    }

    pub fn epoch(&self, group_id: &[u8]) -> Result<u64, MlsError> {
        self.groups
            .get(group_id)
            .map(|group| group.epoch().as_u64())
            .ok_or(MlsError::UnknownGroup)
    }

    /// Serializes the complete MLS key store so the caller can place it inside
    /// the encrypted local database. No plaintext copy should be written to disk.
    pub fn snapshot(&self) -> Result<Vec<u8>, MlsError> {
        let values = self
            .provider
            .storage()
            .values
            .read()
            .map_err(|_| MlsError::Poisoned)?;
        let snapshot = MlsSnapshot {
            values: values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            signer_public: self.signer.public().to_vec(),
            credential: self
                .credential
                .credential
                .tls_serialize_detached()
                .map_err(operation)?,
            credential_signature_key: self.credential.signature_key.as_slice().to_vec(),
            group_ids: self.groups.keys().cloned().collect(),
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&snapshot, &mut bytes).map_err(operation)?;
        Ok(bytes)
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, MlsError> {
        let snapshot: MlsSnapshot = ciborium::from_reader(bytes).map_err(operation)?;
        let provider = OpenMlsRustCrypto::default();
        {
            let mut values = provider
                .storage()
                .values
                .write()
                .map_err(|_| MlsError::Poisoned)?;
            values.extend(snapshot.values);
        }
        let signer = SignatureKeyPair::read(
            provider.storage(),
            &snapshot.signer_public,
            CIPHERSUITE.signature_algorithm(),
        )
        .ok_or(MlsError::MissingSigner)?;
        let mut credential_bytes = snapshot.credential.as_slice();
        let credential = CredentialWithKey {
            credential: Credential::tls_deserialize(&mut credential_bytes).map_err(operation)?,
            signature_key: snapshot.credential_signature_key.into(),
        };
        let mut groups = BTreeMap::new();
        for id in snapshot.group_ids {
            let group = MlsGroup::load(provider.storage(), &GroupId::from_slice(&id))
                .map_err(operation)?
                .ok_or(MlsError::UnknownGroup)?;
            groups.insert(id, group);
        }
        Ok(Self {
            provider,
            signer,
            credential,
            groups,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MlsSnapshot {
    values: Vec<(Vec<u8>, Vec<u8>)>,
    signer_public: Vec<u8>,
    credential: Vec<u8>,
    credential_signature_key: Vec<u8>,
    group_ids: Vec<Vec<u8>>,
}

fn operation(error: impl std::fmt::Debug) -> MlsError {
    MlsError::Operation(format!("{error:?}"))
}

#[derive(Debug, Error)]
pub enum MlsError {
    #[error("MLS group does not exist")]
    UnknownGroup,
    #[error("MLS group already exists")]
    GroupExists,
    #[error("MLS member does not exist")]
    UnknownMember,
    #[error("received an MLS control message rather than application data")]
    ControlMessage,
    #[error("expected an MLS Welcome message")]
    ExpectedWelcome,
    #[error("MLS key store lock was poisoned")]
    Poisoned,
    #[error("MLS snapshot does not contain its signing key")]
    MissingSigner,
    #[error("MLS operation failed: {0}")]
    Operation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_devices_join_and_exchange_forward_secure_messages() {
        let mut alice = MlsClient::new(b"alice-device".to_vec()).expect("alice");
        let mut bob = MlsClient::new(b"bob-device".to_vec()).expect("bob");
        let group_id = b"friends";
        alice.create_group(group_id).expect("create group");
        let bob_key_package = bob.key_package().expect("bob key package");
        let welcome = alice
            .add_member(group_id, &bob_key_package)
            .expect("add bob");
        bob.join_group(group_id, &welcome).expect("join");
        assert_eq!(
            alice.epoch(group_id).expect("alice epoch"),
            bob.epoch(group_id).expect("bob epoch")
        );

        let ciphertext = alice.encrypt(group_id, b"secret hello").expect("encrypt");
        assert!(
            !ciphertext
                .windows(12)
                .any(|window| window == b"secret hello")
        );
        assert_eq!(
            bob.decrypt(group_id, &ciphertext).expect("decrypt"),
            b"secret hello"
        );

        let snapshot = bob.snapshot().expect("snapshot");
        bob = MlsClient::from_snapshot(&snapshot).expect("restore snapshot");
        let after_restart = alice
            .encrypt(group_id, b"after restart")
            .expect("encrypt after restart");
        assert_eq!(
            bob.decrypt(group_id, &after_restart)
                .expect("decrypt after restart"),
            b"after restart"
        );

        let before_charlie = alice
            .encrypt(group_id, b"history before Charlie")
            .expect("encrypt pre-join history");
        assert_eq!(
            bob.decrypt(group_id, &before_charlie)
                .expect("existing member decrypts history"),
            b"history before Charlie"
        );
        let mut charlie = MlsClient::new(b"charlie-device".to_vec()).expect("charlie");
        let charlie_package = charlie.key_package().expect("charlie key package");
        let (charlie_welcome, charlie_commit) = alice
            .add_member_with_commit(group_id, &charlie_package)
            .expect("add charlie");
        assert!(matches!(
            bob.decrypt(group_id, &charlie_commit),
            Err(MlsError::ControlMessage)
        ));
        charlie
            .join_group(group_id, &charlie_welcome)
            .expect("charlie joins");
        assert!(charlie.decrypt(group_id, &before_charlie).is_err());
        let after_charlie = alice
            .encrypt(group_id, b"history after Charlie")
            .expect("encrypt post-join history");
        assert_eq!(
            charlie
                .decrypt(group_id, &after_charlie)
                .expect("new member decrypts new history"),
            b"history after Charlie"
        );

        let removal = alice
            .remove_member(group_id, b"bob-device")
            .expect("remove bob");
        assert!(matches!(
            bob.decrypt(group_id, &removal),
            Err(MlsError::ControlMessage)
        ));
        assert!(alice.epoch(group_id).expect("owner epoch") > 1);
    }
}
