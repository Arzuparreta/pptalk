//! Platform-independent pptalk domain.

mod blob;
mod conversation;
mod crypto;
mod identity;
mod invite;

pub use blob::{BlobError, EncryptedBlob, decrypt_blob, encrypt_blob};
pub use conversation::{
    ApplyError, Conversation, ConversationBuilder, MaterializedMessage, Membership,
};
pub use crypto::{EncryptedPayload, EncryptionError, GroupSecret};
pub use identity::{
    DeviceKeyPair, DeviceRecord, IdentityError, IdentityEvent, IdentityEventKind, IdentityLog,
};
pub use invite::{ContactProofError, sign_invite, verify_invite};
