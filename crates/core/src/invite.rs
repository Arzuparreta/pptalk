use pptalk_protocol::{ContactInvite, DeviceId, ReachabilityRecord, WireEncode};
use thiserror::Error;

use crate::{DeviceKeyPair, IdentityError};

pub fn sign_invite(
    key: &DeviceKeyPair,
    mut invite: ContactInvite,
) -> Result<ContactInvite, ContactProofError> {
    if invite.inviter_device != key.device_id()
        || invite.inviter_device_public_key != key.public_key()
        || invite.reachability.device_id != key.device_id()
    {
        return Err(ContactProofError::DeviceMismatch);
    }
    invite.reachability.signature.clear();
    invite.reachability.signature = key.sign_message(&invite.reachability.to_wire()?);
    invite.signature.clear();
    invite.signature = key.sign_message(&invite.to_wire()?);
    Ok(invite)
}

pub fn verify_invite(invite: &ContactInvite) -> Result<(), ContactProofError> {
    let expected_device =
        DeviceId::from_bytes(*blake3::hash(&invite.inviter_device_public_key).as_bytes());
    if expected_device != invite.inviter_device
        || invite.reachability.device_id != invite.inviter_device
    {
        return Err(ContactProofError::DeviceMismatch);
    }

    let mut reachability: ReachabilityRecord = invite.reachability.clone();
    let reachability_signature = std::mem::take(&mut reachability.signature);
    DeviceKeyPair::verify_message(
        &invite.inviter_device_public_key,
        &reachability.to_wire()?,
        &reachability_signature,
    )?;

    let mut signable = invite.clone();
    let invite_signature = std::mem::take(&mut signable.signature);
    DeviceKeyPair::verify_message(
        &invite.inviter_device_public_key,
        &signable.to_wire()?,
        &invite_signature,
    )?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum ContactProofError {
    #[error("invite device proof does not match")]
    DeviceMismatch,
    #[error("identity signature is invalid: {0}")]
    Identity(#[from] IdentityError),
    #[error("invite encoding failed: {0}")]
    Codec(#[from] pptalk_protocol::CodecError),
}

#[cfg(test)]
mod tests {
    use pptalk_protocol::{IdentityId, PROTOCOL_VERSION};
    use rand::rngs::OsRng;
    use time::{Duration, OffsetDateTime};
    use url::Url;

    use super::*;

    #[test]
    fn signed_invite_detects_address_tampering() {
        let key = DeviceKeyPair::generate(&mut OsRng);
        let expiry = (OffsetDateTime::now_utc() + Duration::hours(1)).unix_timestamp();
        let invite = sign_invite(
            &key,
            ContactInvite {
                version: PROTOCOL_VERSION,
                inviter_identity: IdentityId::from_bytes([8; 32]),
                inviter_device: key.device_id(),
                inviter_device_public_key: key.public_key(),
                display_name: "Alice".into(),
                expires_unix: expiry,
                one_time_secret: [5; 32],
                reachability: ReachabilityRecord {
                    version: PROTOCOL_VERSION,
                    device_id: key.device_id(),
                    expires_unix: expiry,
                    endpoint_id: "endpoint".into(),
                    direct_candidates: vec![],
                    relay_candidates: vec![Url::parse("https://relay.example").expect("url")],
                    mailbox_candidates: vec![],
                    signature: vec![],
                },
                signature: vec![],
            },
        )
        .expect("sign");
        verify_invite(&invite).expect("valid");
        let mut tampered = invite;
        tampered.reachability.endpoint_id = "attacker".into();
        assert!(verify_invite(&tampered).is_err());
    }
}
