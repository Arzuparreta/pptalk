//! Bounded distributed storage for already encrypted pptalk envelopes.
//!
//! This crate deliberately knows nothing about message plaintext, contacts, MLS or calls.
//! It is the viability boundary for Veilid and can be removed without changing Iroh.

use std::{path::Path, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use veilid_core::{
    CRYPTO_KIND_VLD0, DHTSchema, RecordKey, RoutingContext, VeilidAPI, VeilidConfig, api_startup,
};

pub const MAX_DISTRIBUTED_BYTES: usize = 8 * 1024 * 1024;
pub const CONTRIBUTION_LIMIT_MIB: u32 = 64;
pub const CONTRIBUTION_LIMIT_BYTES: usize = CONTRIBUTION_LIMIT_MIB as usize * 1024 * 1024;
pub const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
const VALUE_BYTES: usize = 30 * 1024;
const SUBKEYS_PER_RECORD: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistributedLocator {
    pub version: u16,
    pub record_keys: Vec<String>,
    pub byte_len: u64,
    pub ciphertext_hash: [u8; 32],
    pub expires_unix: i64,
}

impl DistributedLocator {
    pub fn validate(&self, now_unix: i64) -> Result<(), DistributedError> {
        if self.version != 1 {
            return Err(DistributedError::InvalidLocator("unsupported version"));
        }
        if self.expires_unix <= now_unix {
            return Err(DistributedError::Expired);
        }
        let byte_len = usize::try_from(self.byte_len)
            .map_err(|_| DistributedError::InvalidLocator("invalid byte length"))?;
        if byte_len > MAX_DISTRIBUTED_BYTES || self.record_keys.is_empty() {
            return Err(DistributedError::InvalidLocator("invalid payload bounds"));
        }
        let expected_records = chunk_count(byte_len).div_ceil(SUBKEYS_PER_RECORD);
        if self.record_keys.len() != expected_records {
            return Err(DistributedError::InvalidLocator(
                "record count does not match length",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct VeilidBlobStore {
    api: VeilidAPI,
    routing: RoutingContext,
}

impl VeilidBlobStore {
    pub async fn start(
        storage_directory: &Path,
        namespace: &str,
    ) -> Result<Self, DistributedError> {
        std::fs::create_dir_all(storage_directory)?;
        let storage = storage_directory.to_string_lossy();
        let mut config =
            VeilidConfig::new("pptalk", "pptalk", "org", Some(&storage), Some(&storage));
        config.namespace = namespace.to_owned();
        config.network.dht.remote_max_storage_space_mb = CONTRIBUTION_LIMIT_MIB;
        config.network.dht.remote_max_subkey_cache_memory_mb = 16;
        let api = api_startup(Arc::new(|_| {}), config)
            .await
            .map_err(api_error)?;
        api.attach().await.map_err(api_error)?;
        let routing = api.routing_context().map_err(api_error)?;
        Ok(Self { api, routing })
    }

    pub async fn publish(&self, ciphertext: &[u8]) -> Result<DistributedLocator, DistributedError> {
        if ciphertext.is_empty() || ciphertext.len() > MAX_DISTRIBUTED_BYTES {
            return Err(DistributedError::PayloadSize(ciphertext.len()));
        }
        let mut record_keys = Vec::new();
        for record_chunks in ciphertext.chunks(VALUE_BYTES * SUBKEYS_PER_RECORD) {
            let subkey_count = u16::try_from(chunk_count(record_chunks.len()))
                .map_err(|_| DistributedError::PayloadSize(ciphertext.len()))?;
            let descriptor = self
                .routing
                .create_dht_record(
                    CRYPTO_KIND_VLD0,
                    DHTSchema::dflt(subkey_count)
                        .map_err(|error| DistributedError::Veilid(error.to_string()))?,
                    None,
                )
                .await
                .map_err(api_error)?;
            let key = descriptor.key();
            for (subkey, data) in record_chunks.chunks(VALUE_BYTES).enumerate() {
                self.routing
                    .set_dht_value(
                        key.clone(),
                        u32::try_from(subkey)
                            .map_err(|_| DistributedError::PayloadSize(ciphertext.len()))?,
                        data.to_vec(),
                        None,
                    )
                    .await
                    .map_err(api_error)?;
            }
            let flushed = self
                .routing
                .flush_dht_record(key.clone(), Some(Duration::from_secs(30)))
                .await
                .map_err(api_error)?;
            if !flushed {
                return Err(DistributedError::FlushTimeout);
            }
            self.routing
                .close_dht_record(key.clone())
                .await
                .map_err(api_error)?;
            record_keys.push(key.to_string());
        }
        Ok(DistributedLocator {
            version: 1,
            record_keys,
            byte_len: u64::try_from(ciphertext.len())
                .map_err(|_| DistributedError::PayloadSize(ciphertext.len()))?,
            ciphertext_hash: *blake3::hash(ciphertext).as_bytes(),
            expires_unix: (OffsetDateTime::now_utc() + time::Duration::seconds(RETENTION_SECONDS))
                .unix_timestamp(),
        })
    }

    pub async fn retrieve(
        &self,
        locator: &DistributedLocator,
    ) -> Result<Vec<u8>, DistributedError> {
        locator.validate(OffsetDateTime::now_utc().unix_timestamp())?;
        let expected_len = usize::try_from(locator.byte_len)
            .map_err(|_| DistributedError::InvalidLocator("invalid byte length"))?;
        let total_chunks = chunk_count(expected_len);
        let mut output = Vec::with_capacity(expected_len);
        for (record_index, encoded_key) in locator.record_keys.iter().enumerate() {
            let key = encoded_key
                .parse::<RecordKey>()
                .map_err(|_| DistributedError::InvalidLocator("invalid record key"))?;
            let _descriptor = self
                .routing
                .open_dht_record(key.clone(), None)
                .await
                .map_err(api_error)?;
            let record_start = record_index * SUBKEYS_PER_RECORD;
            let record_chunks = (total_chunks - record_start).min(SUBKEYS_PER_RECORD);
            for subkey in 0..record_chunks {
                let value = self
                    .routing
                    .get_dht_value(
                        key.clone(),
                        u32::try_from(subkey)
                            .map_err(|_| DistributedError::InvalidLocator("invalid subkey"))?,
                        true,
                    )
                    .await
                    .map_err(api_error)?
                    .ok_or(DistributedError::MissingChunk {
                        record: record_index,
                        subkey,
                    })?;
                output.extend_from_slice(value.data());
            }
            self.routing
                .close_dht_record(key)
                .await
                .map_err(api_error)?;
        }
        output.truncate(expected_len);
        if blake3::hash(&output).as_bytes() != &locator.ciphertext_hash {
            return Err(DistributedError::HashMismatch);
        }
        Ok(output)
    }

    pub async fn shutdown(self) {
        let _ = self.api.detach().await;
        for _ in 0..100 {
            let detached = self
                .api
                .get_state()
                .await
                .is_ok_and(|state| state.attachment.state.is_detached());
            if detached {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.api.shutdown().await;
    }
}

const fn chunk_count(byte_len: usize) -> usize {
    byte_len.div_ceil(VALUE_BYTES)
}

fn api_error(error: impl std::fmt::Display) -> DistributedError {
    DistributedError::Veilid(error.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum DistributedError {
    #[error("distributed payload must contain 1 byte to 8 MiB, got {0}")]
    PayloadSize(usize),
    #[error("invalid distributed locator: {0}")]
    InvalidLocator(&'static str),
    #[error("distributed copy has expired")]
    Expired,
    #[error("distributed record did not flush before the timeout")]
    FlushTimeout,
    #[error("distributed chunk is missing at record {record}, subkey {subkey}")]
    MissingChunk { record: usize, subkey: usize },
    #[error("distributed payload hash does not match the signed locator")]
    HashMismatch,
    #[error("Veilid operation failed: {0}")]
    Veilid(String),
    #[error("distributed storage path failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_bounds_match_chunk_plan() {
        let size = 2 * 1024 * 1024;
        let records = chunk_count(size).div_ceil(SUBKEYS_PER_RECORD);
        let locator = DistributedLocator {
            version: 1,
            record_keys: (0..records)
                .map(|index| format!("record-{index}"))
                .collect(),
            byte_len: u64::try_from(size).unwrap(),
            ciphertext_hash: [0; 32],
            expires_unix: 100,
        };
        locator.validate(99).expect("valid bounds");
        assert!(matches!(
            locator.validate(100),
            Err(DistributedError::Expired)
        ));
    }

    #[test]
    fn rejects_payloads_over_eight_mebibytes() {
        let locator = DistributedLocator {
            version: 1,
            record_keys: vec!["record".into()],
            byte_len: u64::try_from(MAX_DISTRIBUTED_BYTES + 1).unwrap(),
            ciphertext_hash: [0; 32],
            expires_unix: i64::MAX,
        };
        assert!(matches!(
            locator.validate(0),
            Err(DistributedError::InvalidLocator(_))
        ));
    }
}
