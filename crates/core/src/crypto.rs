use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct GroupSecret([u8; 32]);

impl std::fmt::Debug for GroupSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GroupSecret([REDACTED])")
    }
}

impl GroupSecret {
    pub fn generate() -> Self {
        let key = XChaCha20Poly1305::generate_key(&mut OsRng);
        Self(key.into())
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn encrypt(
        &self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<EncryptedPayload, EncryptionError> {
        let cipher = XChaCha20Poly1305::new((&self.0).into());
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| EncryptionError::Encrypt)?;
        Ok(EncryptedPayload {
            nonce: nonce.into(),
            ciphertext,
        })
    }

    pub fn decrypt(
        &self,
        payload: &EncryptedPayload,
        associated_data: &[u8],
    ) -> Result<Vec<u8>, EncryptionError> {
        let cipher = XChaCha20Poly1305::new((&self.0).into());
        cipher
            .decrypt(
                XNonce::from_slice(&payload.nonce),
                Payload {
                    msg: &payload.ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| EncryptionError::Decrypt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("payload encryption failed")]
    Encrypt,
    #[error("payload authentication or decryption failed")]
    Decrypt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn associated_data_is_authenticated() {
        let key = GroupSecret::generate();
        let payload = key.encrypt(b"private", b"conversation-a").expect("encrypt");
        assert_eq!(
            key.decrypt(&payload, b"conversation-a").expect("decrypt"),
            b"private"
        );
        assert!(key.decrypt(&payload, b"conversation-b").is_err());
    }
}
