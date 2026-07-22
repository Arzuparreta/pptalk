use pptalk_protocol::{BlobManifest, WireDecode, WireEncode};
use thiserror::Error;

use crate::{EncryptedPayload, EncryptionError, GroupSecret};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedBlob {
    pub manifest: BlobManifest,
    pub chunks: Vec<Vec<u8>>,
}

pub fn encrypt_blob(
    secret: &GroupSecret,
    plaintext: &[u8],
    file_name: impl Into<String>,
    media_type: impl Into<String>,
    chunk_size: u32,
) -> Result<EncryptedBlob, BlobError> {
    if chunk_size == 0 || chunk_size > 4 * 1024 * 1024 {
        return Err(BlobError::InvalidChunkSize);
    }
    let chunk_size_usize = usize::try_from(chunk_size).map_err(|_| BlobError::InvalidChunkSize)?;
    let chunks = plaintext
        .chunks(chunk_size_usize)
        .enumerate()
        .map(|(index, chunk)| {
            let aad = chunk_aad(index);
            secret
                .encrypt(chunk, &aad)?
                .to_wire()
                .map_err(BlobError::Codec)
        })
        .collect::<Result<Vec<_>, BlobError>>()?;
    let chunk_hashes = chunks
        .iter()
        .map(|chunk| *blake3::hash(chunk).as_bytes())
        .collect::<Vec<_>>();
    let mut full_ciphertext = Vec::new();
    for chunk in &chunks {
        full_ciphertext.extend_from_slice(chunk);
    }
    Ok(EncryptedBlob {
        manifest: BlobManifest {
            ciphertext_hash: *blake3::hash(&full_ciphertext).as_bytes(),
            byte_len: u64::try_from(plaintext.len()).map_err(|_| BlobError::TooLarge)?,
            chunk_size,
            chunk_hashes,
            media_type: media_type.into(),
            file_name: file_name.into(),
            key_envelope: vec![],
        },
        chunks,
    })
}

pub fn decrypt_blob(secret: &GroupSecret, blob: &EncryptedBlob) -> Result<Vec<u8>, BlobError> {
    if blob.chunks.len() != blob.manifest.chunk_hashes.len() {
        return Err(BlobError::ChunkCount);
    }
    let mut full_ciphertext = Vec::new();
    let mut plaintext = Vec::new();
    for (index, (chunk, expected_hash)) in blob
        .chunks
        .iter()
        .zip(&blob.manifest.chunk_hashes)
        .enumerate()
    {
        if blake3::hash(chunk).as_bytes() != expected_hash {
            return Err(BlobError::HashMismatch(index));
        }
        full_ciphertext.extend_from_slice(chunk);
        let encrypted = EncryptedPayload::from_wire(chunk)?;
        plaintext.extend(secret.decrypt(&encrypted, &chunk_aad(index))?);
    }
    if blake3::hash(&full_ciphertext).as_bytes() != &blob.manifest.ciphertext_hash {
        return Err(BlobError::FullHashMismatch);
    }
    if u64::try_from(plaintext.len()).map_err(|_| BlobError::TooLarge)? != blob.manifest.byte_len {
        return Err(BlobError::LengthMismatch);
    }
    Ok(plaintext)
}

fn chunk_aad(index: usize) -> Vec<u8> {
    let mut aad = b"pptalk-blob-chunk-v1".to_vec();
    aad.extend_from_slice(&index.to_le_bytes());
    aad
}

#[derive(Debug, Error)]
pub enum BlobError {
    #[error("chunk size must be between 1 byte and 4 MiB")]
    InvalidChunkSize,
    #[error("blob is too large for this platform")]
    TooLarge,
    #[error("blob chunk count does not match its manifest")]
    ChunkCount,
    #[error("blob chunk {0} failed hash verification")]
    HashMismatch(usize),
    #[error("full blob ciphertext hash does not match")]
    FullHashMismatch,
    #[error("decrypted blob length does not match its manifest")]
    LengthMismatch,
    #[error("blob encryption failed: {0}")]
    Encryption(#[from] EncryptionError),
    #[error("blob encoding failed: {0}")]
    Codec(#[from] pptalk_protocol::CodecError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_chunked_blob_roundtrips_and_detects_corruption() {
        let secret = GroupSecret::from_bytes([6; 32]);
        let input = vec![42; 200_000];
        let blob = encrypt_blob(&secret, &input, "capture.png", "image/png", 64 * 1024)
            .expect("encrypt blob");
        assert_eq!(blob.chunks.len(), 4);
        assert_eq!(decrypt_blob(&secret, &blob).expect("decrypt blob"), input);
        let mut corrupted = blob;
        corrupted.chunks[1][0] ^= 1;
        assert!(matches!(
            decrypt_blob(&secret, &corrupted),
            Err(BlobError::HashMismatch(1))
        ));
    }
}
