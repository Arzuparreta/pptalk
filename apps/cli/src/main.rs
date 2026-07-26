use std::{
    collections::BTreeSet,
    fmt::Write as FmtWrite,
    fs::OpenOptions,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use argon2::Argon2;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use clap::{Parser, Subcommand};
use pptalk_core::{
    ConversationBuilder, DeviceKeyPair, EncryptedBlob, EncryptedPayload, GroupSecret,
    IdentityEvent, IdentityEventKind, IdentityLog, decrypt_blob, encrypt_blob, sign_invite,
    verify_invite,
};
use pptalk_media::{GstMediaEngine, MediaDeviceKind, MediaEngine};
use pptalk_mls::{MlsClient, MlsError};
use pptalk_network::{PeerAddress, PeerNetwork};
use pptalk_protocol::{
    BlobManifest, CallId, CallSignal, CausalFrontier, ContactInvite, ConversationEvent,
    ConversationId, DeviceId, EventBody, EventId, IdentityId, MediaDatagram, MediaKind,
    MediaSignal, MessageContent, PROTOCOL_VERSION, QualityMode, QualityProfile, ReachabilityRecord,
    TransportEnvelope, WireDecode, WireEncode,
};
use pptalk_storage::{
    CallEventRecord, ConversationSettings, DatabaseKey, DirectMessageRecord, Store,
};
use qrcode::{QrCode, types::Color};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use url::Url;

static LAST_OUTBOX_TIME_MS: AtomicI64 = AtomicI64::new(0);
const MAX_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;

macro_rules! daemon_or_continue {
    ($expression:expr) => {
        match $expression {
            Ok(value) => value,
            Err(error) => {
                emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                continue;
            }
        }
    };
}

#[derive(Debug, Parser)]
#[command(
    name = "pptalk",
    about = "pptalk diagnostics and headless native client"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build and platform capabilities.
    Doctor,
    /// Create a local, serverless identity and stable network endpoint.
    Init {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        name: String,
        /// Optional self-hosted opaque mailbox base URL.
        #[arg(long)]
        mailbox_url: Option<Url>,
    },
    /// Create a signed, one-use contact invitation.
    Invite {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, default_value_t = 3600)]
        expires_seconds: i64,
    },
    /// Verify and save a contact invitation.
    Accept {
        #[arg(long)]
        profile: PathBuf,
        invite: Url,
    },
    /// Send an end-to-end encrypted text message to a saved contact.
    Send {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        contact: String,
        message: String,
    },
    /// Stay online, receive messages and print newline-delimited JSON events.
    Listen {
        #[arg(long)]
        profile: PathBuf,
    },
    /// List saved contacts without accessing the network.
    Contacts {
        #[arg(long)]
        profile: PathBuf,
    },
    /// Authorize a fresh device and print a short-lived capability link.
    LinkDevice {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        label: String,
    },
    /// Create a fresh local profile from an authorized device link.
    ImportDevice {
        #[arg(long)]
        profile: PathBuf,
        link: Url,
    },
    Devices {
        #[arg(long)]
        profile: PathBuf,
    },
    RevokeDevice {
        #[arg(long)]
        profile: PathBuf,
        device_id: String,
        #[arg(long, default_value = "revoked by user")]
        reason: String,
    },
    /// Export an encrypted local identity backup.
    ExportBackup {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, env = "PPTALK_BACKUP_PASSPHRASE", hide_env_values = true)]
        passphrase: String,
    },
    /// Restore a new profile from an encrypted identity backup.
    ImportBackup {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, env = "PPTALK_BACKUP_PASSPHRASE", hide_env_values = true)]
        passphrase: String,
    },
    /// Run the long-lived JSON-lines backend used by the native desktop client.
    Daemon {
        #[arg(long)]
        profile: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Profile {
    version: u16,
    name: String,
    #[serde(default)]
    avatar: Option<String>,
    identity_id: IdentityId,
    device_secret: [u8; 32],
    network_secret: [u8; 32],
    database_key: [u8; 32],
    #[serde(default)]
    database_key_in_keyring: bool,
    #[serde(default)]
    mls_key_package: Vec<u8>,
    #[serde(default)]
    mailbox_url: Option<Url>,
    contacts: Vec<Contact>,
    pending_invites: Vec<PendingInvite>,
    #[serde(default)]
    identity_events: Vec<IdentityEvent>,
    #[serde(default)]
    groups: Vec<GroupProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // Flat fields keep existing profile JSON migratable.
struct Contact {
    name: String,
    #[serde(default)]
    avatar: Option<String>,
    identity_id: IdentityId,
    device_id: DeviceId,
    public_key: [u8; 32],
    address: PeerAddress,
    #[serde(default)]
    mailbox_urls: Vec<Url>,
    shared_secret: [u8; 32],
    verified: bool,
    #[serde(default)]
    manually_verified: bool,
    #[serde(default)]
    mls_key_package: Vec<u8>,
    #[serde(default)]
    identity_events: Vec<IdentityEvent>,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    hide_presence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupProfile {
    id: ConversationId,
    name: String,
    owner: IdentityId,
    #[serde(default)]
    admins: Vec<IdentityId>,
    members: Vec<IdentityId>,
    #[serde(default)]
    member_devices: Vec<DeviceId>,
    #[serde(default)]
    history_since_ms: Vec<(IdentityId, i64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingInvite {
    shared_secret: [u8; 32],
    expires_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatPacket {
    version: u16,
    sender_name: String,
    #[serde(default)]
    sender_avatar: Option<String>,
    sender_identity: IdentityId,
    sender_device: DeviceId,
    sender_public_key: [u8; 32],
    #[serde(default)]
    identity_events: Vec<IdentityEvent>,
    return_address: PeerAddress,
    #[serde(default)]
    mailbox_urls: Vec<Url>,
    #[serde(default)]
    mls_key_package: Vec<u8>,
    sent_at_unix: i64,
    payload: DirectPayload,
    signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DirectPayload {
    DeviceHello,
    DeviceHistory {
        messages: Vec<DeviceHistoryMessage>,
    },
    Message {
        #[serde(default)]
        message_id: [u8; 32],
        body: String,
        #[serde(default)]
        reply_to: Option<[u8; 32]>,
    },
    MessageEdit {
        target: [u8; 32],
        body: String,
    },
    MessageDelete {
        target: [u8; 32],
    },
    DeliveryReceipt {
        target: [u8; 32],
    },
    FileOffer {
        transfer_id: [u8; 32],
        manifest: BlobManifest,
    },
    FileChunk {
        transfer_id: [u8; 32],
        index: u32,
        ciphertext: Vec<u8>,
    },
    GroupWelcome {
        group: GroupProfile,
        welcome: Vec<u8>,
    },
    GroupMessage {
        group_id: ConversationId,
        ciphertext: Vec<u8>,
    },
    GroupCommit {
        group: GroupProfile,
        commit: Vec<u8>,
    },
    GroupProfileUpdate {
        group: GroupProfile,
    },
    GroupDissolve {
        group_id: ConversationId,
    },
    GroupFileOffer {
        group_id: ConversationId,
        transfer_id: [u8; 32],
        encrypted_secret: Vec<u8>,
        manifest: BlobManifest,
        event: Box<ConversationEvent>,
    },
    GroupFileChunk {
        group_id: ConversationId,
        transfer_id: [u8; 32],
        index: u32,
        ciphertext: Vec<u8>,
    },
    GroupSyncRequest {
        group_id: ConversationId,
        frontier: CausalFrontier,
    },
    GroupSyncEvent {
        group_id: ConversationId,
        ciphertext: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceHistoryMessage {
    message_id: [u8; 32],
    peer_identity: IdentityId,
    sender_name: String,
    body: String,
    sent_at_unix: i64,
    outgoing: bool,
    reply_to: Option<[u8; 32]>,
    edited: bool,
    deleted: bool,
    delivery: String,
}

#[derive(Debug, Default)]
struct IncomingFile {
    secret: Option<[u8; 32]>,
    manifest: Option<BlobManifest>,
    group_id: Option<ConversationId>,
    group_event: Option<ConversationEvent>,
    chunks: std::collections::BTreeMap<u32, Vec<u8>>,
}

#[derive(Debug)]
struct TransferControl {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl TransferControl {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}

struct DirectTransferProgress<'a> {
    id: &'a str,
    control: &'a TransferControl,
    device_index: usize,
    device_count: usize,
}

struct QueuedFilePacket {
    conversation_id: ConversationId,
    event_id: EventId,
    recipient: DeviceId,
    envelope: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ActiveCall {
    id: CallId,
    label: String,
    recipients: Vec<Contact>,
    started_at_unix: i64,
    connected_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceBundle {
    version: u16,
    expires_unix: i64,
    name: String,
    #[serde(default)]
    avatar: Option<String>,
    identity_id: IdentityId,
    device_secret: [u8; 32],
    network_secret: [u8; 32],
    database_key: [u8; 32],
    mailbox_url: Option<Url>,
    contacts: Vec<Contact>,
    groups: Vec<GroupProfile>,
    identity_events: Vec<IdentityEvent>,
    mls_snapshot: Option<Vec<u8>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    match Args::parse().command {
        Command::Doctor => {
            doctor();
            Ok(())
        }
        Command::Init {
            profile,
            name,
            mailbox_url,
        } => initialize(&profile, name, mailbox_url),
        Command::Invite {
            profile,
            expires_seconds,
        } => create_invite(&profile, expires_seconds).await,
        Command::Accept { profile, invite } => accept_invite(&profile, &invite),
        Command::Send {
            profile,
            contact,
            message,
        } => send_message(&profile, &contact, &message).await,
        Command::Listen { profile } => listen(&profile).await,
        Command::Contacts { profile } => list_contacts(&profile),
        Command::LinkDevice { profile, label } => link_device(&profile, &label).await,
        Command::ImportDevice { profile, link } => import_device(&profile, &link),
        Command::Devices { profile } => list_devices(&profile),
        Command::RevokeDevice {
            profile,
            device_id,
            reason,
        } => revoke_device(&profile, &device_id, &reason),
        Command::ExportBackup {
            profile,
            output,
            passphrase,
        } => export_identity_backup(&profile, &output, &passphrase),
        Command::ImportBackup {
            profile,
            input,
            passphrase,
        } => import_identity_backup(&profile, &input, &passphrase),
        Command::Daemon { profile } => Box::pin(daemon(&profile)).await,
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum DaemonCommand {
    CheckUpdate,
    Contacts,
    UpdateProfile {
        name: String,
        #[serde(default, deserialize_with = "deserialize_avatar_update")]
        avatar: AvatarUpdate,
    },
    RemoveContact {
        identity_id: String,
    },
    SetContactBlocked {
        identity_id: String,
        blocked: bool,
    },
    SetContactPrivacy {
        identity_id: String,
        hide_presence: bool,
    },
    SetContactVerified {
        identity_id: String,
        verified: bool,
    },
    SetConversationPreference {
        conversation_key: String,
        pinned: bool,
        archived: bool,
        muted: bool,
    },
    RecordConversationActivity {
        conversation_key: String,
        summary: String,
        unread: bool,
        clear_unread: bool,
    },
    Groups,
    Devices,
    MediaCapabilities,
    TestMicrophone,
    SelectMediaDevice {
        kind: String,
        device_id: Option<String>,
    },
    ExportBackup {
        path: PathBuf,
        passphrase: String,
    },
    ProtectLocalSecrets,
    SetMailbox {
        url: Option<Url>,
    },
    LinkDevice {
        label: String,
    },
    RevokeDevice {
        device_id: String,
        reason: Option<String>,
    },
    History {
        contact: String,
    },
    Search {
        query: String,
    },
    Invite {
        expires_seconds: Option<i64>,
    },
    PreviewInvite {
        url: Url,
    },
    Accept {
        url: Url,
    },
    Send {
        contact: String,
        message: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    EditMessage {
        contact: String,
        message_id: String,
        message: String,
    },
    DeleteMessage {
        contact: String,
        message_id: String,
    },
    DeleteMessageLocal {
        contact: String,
        message_id: String,
    },
    SendFile {
        contact: String,
        path: PathBuf,
    },
    CancelTransfer {
        transfer_id: String,
    },
    CreateGroup {
        name: String,
        members: Vec<String>,
    },
    GroupHistory {
        group_id: String,
    },
    GroupSend {
        group_id: String,
        message: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    GroupEditMessage {
        group_id: String,
        message_id: String,
        message: String,
    },
    GroupDeleteMessage {
        group_id: String,
        message_id: String,
    },
    GroupSendFile {
        group_id: String,
        path: PathBuf,
    },
    GroupAddMember {
        group_id: String,
        contact: String,
    },
    GroupRemoveMember {
        group_id: String,
        contact: String,
    },
    GroupSetAdmin {
        group_id: String,
        contact: String,
        admin: bool,
    },
    GroupTransferOwnership {
        group_id: String,
        contact: String,
    },
    GroupDissolve {
        group_id: String,
    },
    StartCall {
        contact: String,
        ring: bool,
    },
    JoinCall {
        contact: String,
        call_id: String,
    },
    RejectCall {
        contact: String,
        call_id: String,
        #[serde(default)]
        missed: bool,
    },
    HoldCall {
        call_id: String,
    },
    ResumeCall {
        call_id: String,
    },
    LeaveCall {
        contact: String,
        call_id: String,
        #[serde(default)]
        missed: bool,
    },
    SetMedia {
        contact: String,
        call_id: String,
        kind: MediaKind,
        enabled: bool,
        profile: Option<QualityProfile>,
    },
    SetParticipantVolume {
        call_id: String,
        device_id: String,
        volume: f64,
    },
    Shutdown,
}

#[derive(Debug, Default)]
enum AvatarUpdate {
    #[default]
    Preserve,
    Set(String),
    Clear,
}

fn deserialize_avatar_update<'de, D>(deserializer: D) -> std::result::Result<AvatarUpdate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Option::<String>::deserialize(deserializer)? {
        Some(value) => AvatarUpdate::Set(value),
        None => AvatarUpdate::Clear,
    })
}

fn doctor() {
    println!("pptalk protocol {PROTOCOL_VERSION}");
    println!(
        "platform: {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("desktop: Qt Quick (no Chromium, no Electron)");
    println!("transport: Iroh QUIC direct + encrypted relay fallback");
    println!("database: SQLCipher");
    println!("media: GStreamer native");
}

fn initialize(path: &Path, name: String, mailbox_url: Option<Url>) -> Result<()> {
    if path.exists() {
        bail!("profile already exists: {}", path.display());
    }
    if name.trim().is_empty() || name.chars().count() > 64 {
        bail!("name must contain 1-64 characters");
    }
    let key = DeviceKeyPair::generate(&mut OsRng);
    let identity =
        IdentityLog::create(&key, "desktop", OffsetDateTime::now_utc().unix_timestamp())?;
    let mut network_secret = [0; 32];
    OsRng.fill_bytes(&mut network_secret);
    let profile = Profile {
        version: PROTOCOL_VERSION,
        name,
        avatar: None,
        identity_id: identity.identity_id(),
        device_secret: key.secret_bytes(),
        network_secret,
        database_key: DatabaseKey::generate().expose_for_profile(),
        database_key_in_keyring: false,
        mls_key_package: vec![],
        mailbox_url,
        contacts: vec![],
        pending_invites: vec![],
        identity_events: identity.events().to_vec(),
        groups: vec![],
    };
    save_profile(path, &profile)?;
    println!("{}", profile.identity_id);
    Ok(())
}

async fn create_invite(path: &Path, expires_seconds: i64) -> Result<()> {
    if !(60..=604_800).contains(&expires_seconds) {
        bail!("expiry must be between 60 seconds and 7 days");
    }
    let mut profile = load_profile(path)?;
    let key = DeviceKeyPair::from_secret_bytes(&profile.device_secret);
    let network = PeerNetwork::start_with_secret(profile.network_secret).await?;
    let address = network.local_address();
    let expires = (OffsetDateTime::now_utc() + Duration::seconds(expires_seconds)).unix_timestamp();
    let mut shared_secret = [0; 32];
    OsRng.fill_bytes(&mut shared_secret);
    let invite = sign_invite(
        &key,
        ContactInvite {
            version: PROTOCOL_VERSION,
            inviter_identity: profile.identity_id,
            inviter_device: key.device_id(),
            inviter_device_public_key: key.public_key(),
            display_name: profile.name.clone(),
            expires_unix: expires,
            one_time_secret: shared_secret,
            reachability: ReachabilityRecord {
                version: PROTOCOL_VERSION,
                device_id: key.device_id(),
                expires_unix: expires,
                endpoint_id: address.endpoint_id,
                direct_candidates: address
                    .direct_addresses
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                relay_candidates: address
                    .relay_urls
                    .iter()
                    .filter_map(|value| Url::parse(value).ok())
                    .collect(),
                mailbox_candidates: profile.mailbox_url.clone().into_iter().collect(),
                signature: vec![],
            },
            signature: vec![],
        },
    )?;
    profile.pending_invites.push(PendingInvite {
        shared_secret,
        expires_unix: expires,
    });
    save_profile(path, &profile)?;
    println!("{}", invite.to_url()?);
    network.shutdown().await?;
    Ok(())
}

fn accept_invite(path: &Path, url: &Url) -> Result<()> {
    let mut profile = load_profile(path)?;
    let invite = ContactInvite::from_url(url, OffsetDateTime::now_utc())?;
    verify_invite(&invite)?;
    let address = PeerAddress {
        endpoint_id: invite.reachability.endpoint_id,
        direct_addresses: invite
            .reachability
            .direct_candidates
            .iter()
            .filter_map(|value| value.parse::<SocketAddr>().ok())
            .collect(),
        relay_urls: invite
            .reachability
            .relay_candidates
            .iter()
            .map(ToString::to_string)
            .collect(),
    };
    upsert_contact(
        &mut profile,
        Contact {
            name: invite.display_name,
            avatar: None,
            identity_id: invite.inviter_identity,
            device_id: invite.inviter_device,
            public_key: invite.inviter_device_public_key,
            address,
            mailbox_urls: invite.reachability.mailbox_candidates,
            shared_secret: invite.one_time_secret,
            verified: true,
            manually_verified: false,
            mls_key_package: vec![],
            identity_events: vec![],
            blocked: false,
            removed: false,
            hide_presence: false,
        },
    );
    save_profile(path, &profile)?;
    Ok(())
}

async fn send_message(path: &Path, name: &str, message: &str) -> Result<()> {
    if message.is_empty() || message.len() > 64 * 1024 {
        bail!("message must contain 1 byte to 64 KiB");
    }
    let profile = load_profile(path)?;
    let recipients = contact_devices(&profile, name)?;
    let key = DeviceKeyPair::from_secret_bytes(&profile.device_secret);
    let network = PeerNetwork::start_with_secret(profile.network_secret).await?;
    let packet = signed_direct_packet(
        &network,
        &key,
        &profile,
        DirectPayload::Message {
            message_id: random_message_id(),
            body: message.into(),
            reply_to: None,
        },
    )?;
    for recipient in &recipients {
        deliver_signed_packet(&network, recipient, &packet).await?;
    }
    network.shutdown().await?;
    Ok(())
}

async fn listen(path: &Path) -> Result<()> {
    let mut profile = load_profile(path)?;
    let network = PeerNetwork::start_with_secret(profile.network_secret).await?;
    println!(
        "{}",
        serde_json::json!({"event":"ready", "address":network.local_address()})
    );
    let mut mailbox_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        tokio::select! {
            incoming = network.receive() => {
                let incoming = incoming?;
                match decrypt_incoming(&profile, &incoming.bytes) {
                    Ok((packet, shared_secret, was_pending)) => {
                        if was_pending {
                            upsert_contact(&mut profile, Contact {
                                name: packet.sender_name.clone(),
                                avatar: None,
                                identity_id: packet.sender_identity,
                                device_id: packet.sender_device,
                                public_key: packet.sender_public_key,
                                address: packet.return_address.clone(),
                                mailbox_urls: packet.mailbox_urls.clone(),
                                shared_secret,
                                verified: true,
                                manually_verified: false,
                                mls_key_package: packet.mls_key_package.clone(),
                                identity_events: packet.identity_events.clone(),
                                blocked: false,
                                removed: false,
                                hide_presence: false,
                            });
                            profile.pending_invites.retain(|pending| pending.shared_secret != shared_secret);
                            save_profile(path, &profile)?;
                        }
                        if refresh_contact_device(&mut profile, &packet, shared_secret)? {
                            save_profile(path, &profile)?;
                        }
                        println!("{}", serde_json::json!({
                            "event":"message",
                            "from":packet.sender_name,
                            "body":direct_message_body(&packet.payload).unwrap_or("[archivo]"),
                            "sent_at":packet.sent_at_unix,
                            "direct_endpoint":incoming.remote_endpoint_id,
                        }));
                    }
                    Err(error) => eprintln!("rejected incoming envelope: {error:#}"),
                }
            }
            _ = mailbox_tick.tick(), if profile.mailbox_url.is_some() => {
                match drain_mailbox(&profile).await {
                    Ok(messages) => for bytes in messages {
                        match decrypt_incoming(&profile, &bytes) {
                            Ok((packet, shared_secret, was_pending)) => {
                                if was_pending {
                                    upsert_contact(&mut profile, Contact {
                                        name: packet.sender_name.clone(),
                                        avatar: None,
                                        identity_id: packet.sender_identity,
                                        device_id: packet.sender_device,
                                        public_key: packet.sender_public_key,
                                        address: packet.return_address.clone(),
                                        mailbox_urls: packet.mailbox_urls.clone(),
                                        shared_secret,
                                        verified: true,
                                        manually_verified: false,
                                        mls_key_package: packet.mls_key_package.clone(),
                                        identity_events: packet.identity_events.clone(),
                                        blocked: false,
                                        removed: false,
                                        hide_presence: false,
                                    });
                                    profile.pending_invites.retain(|pending| pending.shared_secret != shared_secret);
                                    save_profile(path, &profile)?;
                                }
                                if refresh_contact_device(&mut profile, &packet, shared_secret)? {
                                    save_profile(path, &profile)?;
                                }
                                println!("{}", serde_json::json!({
                                    "event":"message", "from":packet.sender_name,
                                    "body":direct_message_body(&packet.payload).unwrap_or("[archivo]"), "sent_at":packet.sent_at_unix,
                                    "delivery":"mailbox"
                                }));
                            }
                            Err(error) => eprintln!("rejected mailbox envelope: {error:#}"),
                        }
                    },
                    Err(error) => tracing::warn!(%error, "mailbox poll failed"),
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                break;
            }
        }
    }
    network.shutdown().await?;
    Ok(())
}

#[allow(clippy::needless_continue)]
async fn daemon(path: &Path) -> Result<()> {
    let mut profile = load_profile(path)?;
    let store = Store::open(
        path.with_extension("history.sqlite3"),
        &DatabaseKey::from_bytes(profile.database_key),
    )?;
    let key = DeviceKeyPair::from_secret_bytes(&profile.device_secret);
    let network = PeerNetwork::start_with_secret(profile.network_secret).await?;
    let media = Arc::new(GstMediaEngine::new()?);
    let mut active_call: Option<ActiveCall> = None;
    let mut held_call: Option<ActiveCall> = None;
    let mut media_tasks: Vec<(MediaKind, tokio::task::JoinHandle<()>)> = Vec::new();
    let (transfer_done_sender, mut transfer_done_receiver) =
        tokio::sync::mpsc::unbounded_channel::<String>();
    let mut transfer_tasks = std::collections::BTreeMap::<
        String,
        (Arc<TransferControl>, tokio::task::JoinHandle<()>),
    >::new();
    let mut incoming_files = std::collections::BTreeMap::<[u8; 32], IncomingFile>::new();
    let mls_snapshot_id = ConversationId::from_bytes([0x6d; 32]);
    let mut mls = match store.load_mls_state(mls_snapshot_id)? {
        Some(snapshot) => MlsClient::from_snapshot(&snapshot)?,
        None => MlsClient::new(key.device_id().as_bytes().to_vec())?,
    };
    profile.mls_key_package = mls.key_package()?;
    let local_identity_id = profile.identity_id;
    let local_identity_events = profile.identity_events.clone();
    remove_revoked_devices_from_owned_groups(
        &network,
        &key,
        &store,
        &mut profile,
        &mut mls,
        local_identity_id,
        &local_identity_events,
    )
    .await?;
    store.save_mls_state(mls_snapshot_id, &mls.snapshot()?, now_millis())?;
    save_profile(path, &profile)?;
    emit_json(&serde_json::json!({
        "event":"ready",
        "identity_id":profile.identity_id,
        "name":profile.name,
        "avatar":profile.avatar,
        "secure_storage":profile.database_key_in_keyring,
        "address":network.local_address()
    }))?;
    emit_contacts(&profile)?;
    emit_groups(&profile)?;
    emit_devices(&profile)?;
    emit_conversation_settings(&store)?;
    for recipient in &profile.contacts {
        if let Err(error) = deliver_payload(
            &network,
            &key,
            &profile,
            recipient,
            DirectPayload::DeviceHello,
        )
        .await
        {
            tracing::debug!(%error, contact = %recipient.name, "device announcement deferred");
        }
    }
    for group in &profile.groups {
        let frontier = conversation_frontier(&store, group.id)?;
        for recipient in group_remote_contacts(&profile, group, key.device_id()) {
            if let Err(error) = deliver_payload(
                &network,
                &key,
                &profile,
                &recipient,
                DirectPayload::GroupSyncRequest {
                    group_id: group.id,
                    frontier: frontier.clone(),
                },
            )
            .await
            {
                tracing::debug!(%error, group = %group.id, "group sync request deferred");
            }
        }
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut mailbox_tick = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut outbox_tick = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                let command = match serde_json::from_str::<DaemonCommand>(&line) {
                    Ok(command) => command,
                    Err(error) => {
                        emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                        continue;
                    }
                };
                match command {
                    DaemonCommand::CheckUpdate => {
                        match check_for_update().await {
                            Ok(update) => emit_json(&update)?,
                            Err(error) => tracing::debug!(%error, "update check failed"),
                        }
                    }
                    DaemonCommand::Contacts => emit_contacts(&profile)?,
                    DaemonCommand::UpdateProfile { name, avatar } => {
                        let name = name.trim();
                        if name.is_empty() || name.chars().count() > 64 {
                            emit_json(&serde_json::json!({"event":"error", "message":"name must contain 1-64 characters"}))?;
                            continue;
                        }
                        if matches!(&avatar, AvatarUpdate::Set(value) if value.len() > 512 * 1024) {
                            emit_json(&serde_json::json!({"event":"error", "message":"avatar exceeds 512 KiB"}))?;
                            continue;
                        }
                        profile.name = name.to_owned();
                        match avatar {
                            AvatarUpdate::Set(value) => {
                                profile.avatar = (!value.is_empty()).then_some(value);
                            }
                            AvatarUpdate::Clear => profile.avatar = None,
                            AvatarUpdate::Preserve => {}
                        }
                        save_profile(path, &profile)?;
                        for recipient in profile.contacts.iter().filter(|contact| {
                            contact.identity_id != profile.identity_id
                                && !contact.removed
                                && !contact.blocked
                        }) {
                            if let Err(error) = deliver_payload(
                                &network,
                                &key,
                                &profile,
                                recipient,
                                DirectPayload::DeviceHello,
                            )
                            .await
                            {
                                tracing::debug!(%error, contact = %recipient.name, "profile update deferred");
                            }
                        }
                        emit_json(&serde_json::json!({"event":"profile", "name":profile.name, "avatar":profile.avatar}))?;
                    }
                    DaemonCommand::RemoveContact { identity_id } => {
                        let identity = daemon_or_continue!(identity_id.parse::<IdentityId>().context("invalid identity id"));
                        let mut changed = false;
                        for contact in profile.contacts.iter_mut().filter(|contact| contact.identity_id == identity) {
                            contact.removed = true;
                            changed = true;
                        }
                        if changed {
                            save_profile(path, &profile)?;
                            emit_contacts(&profile)?;
                        }
                    }
                    DaemonCommand::SetContactBlocked { identity_id, blocked } => {
                        let identity = daemon_or_continue!(identity_id.parse::<IdentityId>().context("invalid identity id"));
                        let mut changed = false;
                        for contact in profile.contacts.iter_mut().filter(|contact| contact.identity_id == identity) {
                            contact.blocked = blocked;
                            changed = true;
                        }
                        if changed {
                            save_profile(path, &profile)?;
                            emit_contacts(&profile)?;
                        }
                    }
                    DaemonCommand::SetContactPrivacy { identity_id, hide_presence } => {
                        let identity = daemon_or_continue!(identity_id.parse::<IdentityId>().context("invalid identity id"));
                        let mut changed = false;
                        for contact in profile.contacts.iter_mut().filter(|contact| contact.identity_id == identity) {
                            contact.hide_presence = hide_presence;
                            changed = true;
                        }
                        if changed {
                            save_profile(path, &profile)?;
                            emit_contacts(&profile)?;
                        }
                    }
                    DaemonCommand::SetContactVerified { identity_id, verified } => {
                        let identity = daemon_or_continue!(
                            identity_id.parse::<IdentityId>().context("invalid identity id")
                        );
                        let mut changed = false;
                        for contact in profile.contacts.iter_mut()
                            .filter(|contact| contact.identity_id == identity)
                        {
                            contact.manually_verified = verified;
                            changed = true;
                        }
                        if changed {
                            save_profile(path, &profile)?;
                            emit_contacts(&profile)?;
                        }
                    }
                    DaemonCommand::SetConversationPreference { conversation_key, pinned, archived, muted } => {
                        if conversation_key.trim().is_empty() || conversation_key.len() > 128 {
                            emit_json(&serde_json::json!({"event":"error", "message":"invalid conversation key"}))?;
                            continue;
                        }
                        let previous = store.load_conversation_settings()?.into_iter()
                            .find(|settings| settings.conversation_key == conversation_key);
                        store.save_conversation_settings(&ConversationSettings {
                            conversation_key,
                            pinned,
                            archived,
                            muted_until_unix: muted.then_some(i64::MAX),
                            unread_count: previous.as_ref().map_or(0, |settings| settings.unread_count),
                            last_summary: previous.as_ref().map_or_else(String::new, |settings| settings.last_summary.clone()),
                            last_activity_unix: previous.as_ref().map_or(0, |settings| settings.last_activity_unix),
                            notification_preview: previous.as_ref().is_none_or(|settings| settings.notification_preview),
                        })?;
                        emit_conversation_settings(&store)?;
                    }
                    DaemonCommand::RecordConversationActivity {
                        conversation_key, summary, unread, clear_unread
                    } => {
                        if conversation_key.trim().is_empty() || conversation_key.len() > 128 {
                            continue;
                        }
                        let previous = store.load_conversation_settings()?.into_iter()
                            .find(|settings| settings.conversation_key == conversation_key);
                        let unread_count = if clear_unread {
                            0
                        } else {
                            previous.as_ref().map_or(u32::from(unread), |settings| {
                                settings.unread_count.saturating_add(u32::from(unread))
                            })
                        };
                        store.save_conversation_settings(&ConversationSettings {
                            conversation_key,
                            pinned: previous.as_ref().is_some_and(|settings| settings.pinned),
                            archived: previous.as_ref().is_some_and(|settings| settings.archived),
                            muted_until_unix: previous.as_ref().and_then(|settings| settings.muted_until_unix),
                            unread_count,
                            last_summary: if summary.is_empty() {
                                previous.as_ref().map_or_else(String::new, |settings| settings.last_summary.clone())
                            } else {
                                summary
                            },
                            last_activity_unix: if clear_unread && previous.is_some() {
                                previous.as_ref().map_or(0, |settings| settings.last_activity_unix)
                            } else {
                                OffsetDateTime::now_utc().unix_timestamp()
                            },
                            notification_preview: previous.as_ref().is_none_or(|settings| settings.notification_preview),
                        })?;
                        emit_conversation_settings(&store)?;
                    }
                    DaemonCommand::Groups => emit_groups(&profile)?,
                    DaemonCommand::Devices => emit_devices(&profile)?,
                    DaemonCommand::MediaCapabilities => {
                        let capabilities = daemon_or_continue!(media.capabilities().await);
                        let devices = capabilities.devices.into_iter().map(|device| {
                            let kind = match device.kind {
                                MediaDeviceKind::AudioInput => "audio_input",
                                MediaDeviceKind::AudioOutput => "audio_output",
                                MediaDeviceKind::Camera => "camera",
                                MediaDeviceKind::Screen => "screen",
                                MediaDeviceKind::Window => "window",
                            };
                            serde_json::json!({
                                "id":device.id, "label":device.label, "kind":kind,
                                "default":device.is_default
                            })
                        }).collect::<Vec<_>>();
                        emit_json(&serde_json::json!({
                            "event":"media_capabilities", "devices":devices,
                            "encoders":capabilities.encoders,
                            "decoders":capabilities.decoders,
                            "zero_copy":capabilities.zero_copy_backends
                        }))?;
                    }
                    DaemonCommand::TestMicrophone => {
                        if active_call.is_some() {
                            emit_json(&serde_json::json!({
                                "event":"error",
                                "message":"the microphone test is unavailable during a call"
                            }))?;
                            continue;
                        }
                        let profile = default_media_profile(MediaKind::Voice);
                        match media.publish(MediaKind::Voice, profile).await {
                            Ok(()) => {
                                let detected = tokio::time::timeout(
                                    std::time::Duration::from_secs(3),
                                    async {
                                        loop {
                                            if media
                                                .next_rtp_packet(MediaKind::Voice)
                                                .await?
                                                .is_some()
                                            {
                                                return Ok::<_, anyhow::Error>(true);
                                            }
                                        }
                                    },
                                )
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .unwrap_or(false);
                                media.unpublish(MediaKind::Voice).await.ok();
                                emit_json(&serde_json::json!({
                                    "event":"microphone_test", "detected":detected
                                }))?;
                            }
                            Err(error) => emit_json(&serde_json::json!({
                                "event":"error", "message":error.to_string()
                            }))?,
                        }
                    }
                    DaemonCommand::SelectMediaDevice { kind, device_id } => {
                        let kind = match kind.as_str() {
                            "audio_input" => MediaDeviceKind::AudioInput,
                            "audio_output" => MediaDeviceKind::AudioOutput,
                            "camera" => MediaDeviceKind::Camera,
                            _ => {
                                emit_json(&serde_json::json!({
                                    "event":"error",
                                    "message":"unsupported media device kind"
                                }))?;
                                continue;
                            }
                        };
                        match media.select_device(kind, device_id.clone()).await {
                            Ok(()) => emit_json(&serde_json::json!({
                                "event":"media_device_selected", "kind":kind_name(kind),
                                "device_id":device_id
                            }))?,
                            Err(error) => emit_json(&serde_json::json!({
                                "event":"error", "message":error.to_string()
                            }))?,
                        }
                    }
                    DaemonCommand::ExportBackup { path: output, passphrase } => {
                        match export_identity_backup(path, &output, &passphrase) {
                            Ok(()) => emit_json(&serde_json::json!({
                                "event":"backup_exported", "path":output
                            }))?,
                            Err(error) => emit_json(&serde_json::json!({
                                "event":"error", "message":error.to_string()
                            }))?,
                        }
                    }
                    DaemonCommand::ProtectLocalSecrets => {
                        match protect_database_key(path, &mut profile) {
                            Ok(()) => emit_json(&serde_json::json!({
                                "event":"secure_storage", "enabled":true
                            }))?,
                            Err(error) => emit_json(&serde_json::json!({
                                "event":"error", "message":error.to_string()
                            }))?,
                        }
                    }
                    DaemonCommand::SetMailbox { url } => {
                        if let Some(candidate) = &url {
                            daemon_or_continue!(mailbox_endpoint(candidate, &[0; 32]));
                        }
                        profile.mailbox_url = url;
                        save_profile(path, &profile)?;
                        emit_json(&serde_json::json!({"event":"mailbox_configured", "url":profile.mailbox_url}))?;
                    }
                    DaemonCommand::LinkDevice { label } => {
                        let url = daemon_or_continue!(make_device_link(
                            &mut profile, &label, network.local_address()
                        ));
                        save_profile(path, &profile)?;
                        emit_json(&serde_json::json!({"event":"device_link", "url":url}))?;
                        emit_devices(&profile)?;
                    }
                    DaemonCommand::RevokeDevice { device_id, reason } => {
                        daemon_or_continue!(revoke_profile_device(
                            &mut profile,
                            &device_id,
                            reason.as_deref().unwrap_or("revoked by user"),
                        ));
                        let identity_id = profile.identity_id;
                        let identity_events = profile.identity_events.clone();
                        let groups_changed = remove_revoked_devices_from_owned_groups(
                            &network, &key, &store, &mut profile, &mut mls,
                            identity_id, &identity_events,
                        ).await?;
                        if groups_changed {
                            persist_mls(&store, mls_snapshot_id, &mls)?;
                            emit_groups(&profile)?;
                        }
                        save_profile(path, &profile)?;
                        emit_devices(&profile)?;
                        for recipient in profile.contacts.iter().filter(|contact| {
                            contact.identity_id != profile.identity_id
                        }) {
                            deliver_payload(
                                &network, &key, &profile, recipient, DirectPayload::DeviceHello,
                            ).await.ok();
                        }
                    }
                    DaemonCommand::History { contact } => {
                        let Some(peer) = profile.contacts.iter().find(|item| item.name.eq_ignore_ascii_case(&contact)) else {
                            emit_json(&serde_json::json!({"event":"error", "message":"contact not found"}))?;
                            continue;
                        };
                        emit_history(&store, peer)?;
                        emit_call_history(&store, &peer.name)?;
                    }
                    DaemonCommand::Search { query } => {
                        if query.trim().is_empty() {
                            emit_json(&serde_json::json!({"event":"search_results", "query":query, "results":[]}))?;
                            continue;
                        }
                        let direct = daemon_or_continue!(store.search_direct_messages(query.trim(), 100));
                        let mut results = direct.iter().map(|message| {
                            let mut value = direct_message_json(message);
                            if let Some(object) = value.as_object_mut() {
                                object.insert("conversation_type".into(), "direct".into());
                                object.insert("conversation_key".into(), message.peer_identity.to_string().into());
                            }
                            value
                        }).collect::<Vec<_>>();
                        let needle = query.trim().to_lowercase();
                        for group in &profile.groups {
                            let messages = daemon_or_continue!(materialize_group_messages(
                                &store, &profile, group.id
                            ));
                            for mut message in messages {
                                let matches = message.get("body").and_then(serde_json::Value::as_str)
                                    .is_some_and(|body| body.to_lowercase().contains(&needle));
                                let deleted = message.get("deleted").and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false);
                                if !matches || deleted { continue; }
                                if let Some(object) = message.as_object_mut() {
                                    let author = object.get("author").and_then(serde_json::Value::as_str)
                                        .unwrap_or_default().to_owned();
                                    object.insert("author".into(), format!("{} · {author}", group.name).into());
                                    object.insert("conversation_type".into(), "group".into());
                                    object.insert("conversation_key".into(), group.id.to_string().into());
                                }
                                results.push(message);
                            }
                        }
                        results.sort_by_key(|message| std::cmp::Reverse(
                            message.get("sent_at").and_then(serde_json::Value::as_i64).unwrap_or_default()
                        ));
                        results.truncate(100);
                        emit_json(&serde_json::json!({
                            "event":"search_results", "query":query,
                            "results":results
                        }))?;
                    }
                    DaemonCommand::Invite { expires_seconds } => {
                        let seconds = expires_seconds.unwrap_or(3600);
                        if !(60..=604_800).contains(&seconds) {
                            emit_json(&serde_json::json!({"event":"error", "message":"expiry must be between 60 seconds and 7 days"}))?;
                            continue;
                        }
                        let expires = (OffsetDateTime::now_utc() + Duration::seconds(seconds)).unix_timestamp();
                        let mut shared_secret = [0; 32];
                        OsRng.fill_bytes(&mut shared_secret);
                        let address = network.local_address();
                        let invite = sign_invite(&key, ContactInvite {
                            version: PROTOCOL_VERSION,
                            inviter_identity: profile.identity_id,
                            inviter_device: key.device_id(),
                            inviter_device_public_key: key.public_key(),
                            display_name: profile.name.clone(),
                            expires_unix: expires,
                            one_time_secret: shared_secret,
                            reachability: ReachabilityRecord {
                                version: PROTOCOL_VERSION,
                                device_id: key.device_id(),
                                expires_unix: expires,
                                endpoint_id: address.endpoint_id,
                                direct_candidates: address.direct_addresses.iter().map(ToString::to_string).collect(),
                                relay_candidates: address.relay_urls.iter().filter_map(|value| Url::parse(value).ok()).collect(),
                                mailbox_candidates: profile.mailbox_url.clone().into_iter().collect(),
                                signature: vec![],
                            },
                            signature: vec![],
                        })?;
                        profile.pending_invites.push(PendingInvite { shared_secret, expires_unix: expires });
                        save_profile(path, &profile)?;
                        let invite_url = invite.to_url()?;
                        emit_json(&serde_json::json!({
                            "event":"invite", "url":invite_url, "expires_unix":expires,
                            "qr_svg":qr_svg(invite_url.as_str())?
                        }))?;
                    }
                    DaemonCommand::PreviewInvite { url } => {
                        let invite = daemon_or_continue!(ContactInvite::from_url(
                            &url, OffsetDateTime::now_utc()
                        ));
                        daemon_or_continue!(verify_invite(&invite));
                        emit_json(&serde_json::json!({
                            "event":"invite_preview", "url":url,
                            "name":invite.display_name, "expires_unix":invite.expires_unix,
                            "identity_id":invite.inviter_identity
                        }))?;
                    }
                    DaemonCommand::Accept { url } => {
                        let invite = daemon_or_continue!(ContactInvite::from_url(
                            &url, OffsetDateTime::now_utc()
                        ));
                        daemon_or_continue!(verify_invite(&invite));
                        let address = PeerAddress {
                            endpoint_id: invite.reachability.endpoint_id,
                            direct_addresses: invite.reachability.direct_candidates.iter().filter_map(|value| value.parse().ok()).collect(),
                            relay_urls: invite.reachability.relay_candidates.iter().map(ToString::to_string).collect(),
                        };
                        let accepted_identity = invite.inviter_identity;
                        upsert_contact(&mut profile, Contact {
                            name: invite.display_name,
                            avatar: None,
                            identity_id: invite.inviter_identity,
                            device_id: invite.inviter_device,
                            public_key: invite.inviter_device_public_key,
                            address,
                            mailbox_urls: invite.reachability.mailbox_candidates,
                            shared_secret: invite.one_time_secret,
                            verified: true,
                            manually_verified: false,
                            mls_key_package: vec![],
                            identity_events: vec![],
                            blocked: false,
                            removed: false,
                            hide_presence: false,
                        });
                        save_profile(path, &profile)?;
                        emit_contacts(&profile)?;
                        if let Some(accepted) = profile.contacts.iter()
                            .find(|contact| contact.identity_id == accepted_identity)
                            .cloned()
                        {
                            let conversation_id = direct_conversation_id(
                                profile.identity_id, accepted.identity_id
                            );
                            deliver_payload_durable(
                                &network, &key, &profile, &store, conversation_id,
                                &accepted, DirectPayload::DeviceHello
                            ).await?;
                        }
                    }
                    DaemonCommand::Send { contact, message, reply_to } => {
                        let recipients = match contact_devices(&profile, &contact) {
                            Ok(recipients) => recipients,
                            Err(error) => {
                                emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                                continue;
                            }
                        };
                        if message.is_empty() || message.len() > 64 * 1024 {
                            emit_json(&serde_json::json!({"event":"error", "message":"message must contain 1 byte to 64 KiB"}))?;
                            continue;
                        }
                        let message_id = random_message_id();
                        let reply_to = match reply_to.as_deref().map(parse_message_id).transpose() {
                            Ok(reply_to) => reply_to,
                            Err(error) => {
                                emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                                continue;
                            }
                        };
                        let mut packet = ChatPacket {
                            version: PROTOCOL_VERSION,
                            sender_name: profile.name.clone(),
                            sender_avatar: profile.avatar.clone(),
                            sender_identity: profile.identity_id,
                            sender_device: key.device_id(),
                            sender_public_key: key.public_key(),
                            identity_events: profile.identity_events.clone(),
                            return_address: network.local_address(),
                            mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
                            mls_key_package: profile.mls_key_package.clone(),
                            sent_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
                            payload: DirectPayload::Message {
                                message_id,
                                body: message.clone(),
                                reply_to,
                            },
                            signature: vec![],
                        };
                        packet.signature = key.sign_message(&packet.to_wire()?);
                        let delivery = async {
                            let mut route = "direct";
                            let conversation_id = direct_conversation_id(
                                profile.identity_id,
                                recipients[0].identity_id,
                            );
                            for recipient in &recipients {
                                match deliver_signed_packet_durable(
                                    &network,
                                    &store,
                                    conversation_id,
                                    recipient,
                                    &packet,
                                )
                                .await?
                                {
                                    "queued" => route = "queued",
                                    "mailbox" if route == "direct" => route = "mailbox",
                                    _ => {}
                                }
                            }
                            Ok::<_, anyhow::Error>(route)
                        }.await;
                        match delivery {
                            Ok(delivery) => {
                                store.save_direct_message(&DirectMessageRecord {
                                    message_id,
                                    peer_identity: recipients[0].identity_id,
                                    sender_name: profile.name.clone(),
                                    body: message.clone(),
                                    sent_at_unix: packet.sent_at_unix,
                                    outgoing: true,
                                    reply_to,
                                    edited: false,
                                    deleted: false,
                                    delivery: delivery.to_owned(),
                                    file_path: None,
                                })?;
                                emit_json(&serde_json::json!({"event":"message_sent", "message_id":hex::encode(message_id), "to":recipients[0].name, "devices":recipients.len(), "body":message, "sent_at":packet.sent_at_unix, "delivery":delivery, "reply_to":reply_to.map(hex::encode)}))?;
                            }
                            Err(error) => emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?,
                        }
                    }
                    DaemonCommand::EditMessage { contact, message_id, message } => {
                        if message.is_empty() || message.len() > 64 * 1024 {
                            emit_json(&serde_json::json!({"event":"error", "message":"message must contain 1 byte to 64 KiB"}))?;
                            continue;
                        }
                        let target = daemon_or_continue!(parse_message_id(&message_id));
                        let recipients = daemon_or_continue!(contact_devices(&profile, &contact));
                        let conversation_id = direct_conversation_id(profile.identity_id, recipients[0].identity_id);
                        let packet = daemon_or_continue!(signed_direct_packet(
                            &network, &key, &profile,
                            DirectPayload::MessageEdit { target, body: message.clone() },
                        ));
                        let mut sent = false;
                        for recipient in &recipients {
                            sent |= deliver_signed_packet_durable(
                                &network, &store, conversation_id, recipient, &packet,
                            ).await.is_ok();
                        }
                        if sent && store.update_direct_message(
                            target, recipients[0].identity_id, true, Some(&message), false,
                        )? {
                            emit_json(&serde_json::json!({"event":"message_edited", "contact":contact, "message_id":message_id, "body":message}))?;
                        } else {
                            emit_json(&serde_json::json!({"event":"error", "message":"message not found or could not be queued"}))?;
                        }
                    }
                    DaemonCommand::DeleteMessage { contact, message_id } => {
                        let target = daemon_or_continue!(parse_message_id(&message_id));
                        let recipients = daemon_or_continue!(contact_devices(&profile, &contact));
                        let conversation_id = direct_conversation_id(profile.identity_id, recipients[0].identity_id);
                        let packet = daemon_or_continue!(signed_direct_packet(
                            &network, &key, &profile, DirectPayload::MessageDelete { target },
                        ));
                        let mut sent = false;
                        for recipient in &recipients {
                            sent |= deliver_signed_packet_durable(
                                &network, &store, conversation_id, recipient, &packet,
                            ).await.is_ok();
                        }
                        if sent && store.update_direct_message(
                            target, recipients[0].identity_id, true, None, true,
                        )? {
                            emit_json(&serde_json::json!({"event":"message_deleted", "contact":contact, "message_id":message_id}))?;
                        } else {
                            emit_json(&serde_json::json!({"event":"error", "message":"message not found or could not be queued"}))?;
                        }
                    }
                    DaemonCommand::DeleteMessageLocal { contact, message_id } => {
                        let target = daemon_or_continue!(parse_message_id(&message_id));
                        let recipients = daemon_or_continue!(contact_devices(&profile, &contact));
                        if store.delete_direct_message_local(target, recipients[0].identity_id)? {
                            emit_json(&serde_json::json!({"event":"message_deleted", "contact":contact, "message_id":message_id}))?;
                        } else {
                            emit_json(&serde_json::json!({"event":"error", "message":"message not found"}))?;
                        }
                    }
                    DaemonCommand::SendFile { contact, path: file_path } => {
                        let recipients = match contact_devices(&profile, &contact) {
                            Ok(recipients) => recipients,
                            Err(error) => {
                                emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                                continue;
                            }
                        };
                        let metadata = match std::fs::metadata(&file_path)
                            .with_context(|| format!("read {}", file_path.display()))
                        {
                            Ok(metadata) if metadata.is_file() => metadata,
                            Ok(_) => {
                                emit_json(&serde_json::json!({
                                    "event":"error",
                                    "message":"attachment must be a regular file"
                                }))?;
                                continue;
                            }
                            Err(error) => {
                                emit_json(&serde_json::json!({
                                    "event":"error", "message":error.to_string()
                                }))?;
                                continue;
                            }
                        };
                        if metadata.len() > MAX_ATTACHMENT_BYTES {
                            emit_json(&serde_json::json!({
                                "event":"error",
                                "message":"attachment exceeds the 512 MiB client limit"
                            }))?;
                            continue;
                        }
                        let request_id = uuid::Uuid::new_v4().to_string();
                        let control = Arc::new(TransferControl::new());
                        let task_control = Arc::clone(&control);
                        let task_network = network.clone();
                        let task_key = key.clone();
                        let task_profile = profile.clone();
                        let task_profile_path = path.to_owned();
                        let task_request_id = request_id.clone();
                        let task_done_sender = transfer_done_sender.clone();
                        let task = tokio::spawn(async move {
                            let result = async {
                                let device_count = recipients.len();
                                let peer_identity = recipients
                                    .first()
                                    .context("contact has no active devices")?
                                    .identity_id;
                                let peer_name = recipients
                                    .first()
                                    .context("contact has no active devices")?
                                    .name
                                    .clone();
                                let mut first = None;
                                let mut route = "direct";
                                let mut queued = Vec::new();
                                for (device_index, recipient) in recipients.iter().enumerate() {
                                    let (transfer_id, file_name, byte_len, delivery) =
                                        send_file_packets(
                                            &task_network,
                                            &task_key,
                                            &task_profile,
                                            recipient,
                                            &file_path,
                                            &mut queued,
                                            DirectTransferProgress {
                                                id: &task_request_id,
                                                control: &task_control,
                                                device_index,
                                                device_count,
                                            },
                                        )
                                        .await?;
                                    match delivery {
                                        "queued" => route = "queued",
                                        "mailbox" if route == "direct" => route = "mailbox",
                                        _ => {}
                                    }
                                    first.get_or_insert((transfer_id, file_name, byte_len));
                                }
                                let (transfer_id, file_name, byte_len) =
                                    first.context("contact has no active devices")?;
                                if task_control.is_cancelled() {
                                    bail!("transfer cancelled");
                                }
                                let task_store = Store::open(
                                    task_profile_path.with_extension("history.sqlite3"),
                                    &DatabaseKey::from_bytes(task_profile.database_key),
                                )?;
                                for packet in queued {
                                    task_store.enqueue(
                                        packet.conversation_id,
                                        packet.event_id,
                                        packet.recipient,
                                        &packet.envelope,
                                        next_outbox_time(),
                                    )?;
                                }
                                let sent_at = OffsetDateTime::now_utc().unix_timestamp();
                                task_store.save_direct_message(&DirectMessageRecord {
                                    message_id: transfer_id,
                                    peer_identity,
                                    sender_name: task_profile.name.clone(),
                                    body: format!("📎 {file_name}"),
                                    sent_at_unix: sent_at,
                                    outgoing: true,
                                    reply_to: None,
                                    edited: false,
                                    deleted: false,
                                    delivery: route.to_owned(),
                                    file_path: Some(file_path.to_string_lossy().into_owned()),
                                })?;
                                Ok::<_, anyhow::Error>((
                                    peer_name,
                                    device_count,
                                    file_name,
                                    byte_len,
                                    sent_at,
                                    route,
                                ))
                            }
                            .await;
                            if task_control.is_cancelled() {
                                let _ = emit_json(&serde_json::json!({
                                    "event":"transfer_cancelled",
                                    "transfer_id":task_request_id
                                }));
                            } else {
                                match result {
                                    Ok((
                                        peer_name,
                                        device_count,
                                        file_name,
                                        byte_len,
                                        sent_at,
                                        delivery,
                                    )) => {
                                        let _ = emit_json(&serde_json::json!({
                                            "event":"file_sent", "to":peer_name,
                                            "devices":device_count, "file_name":file_name,
                                            "byte_len":byte_len, "sent_at":sent_at,
                                            "delivery":delivery,
                                            "transfer_id":task_request_id
                                        }));
                                    }
                                    Err(error) => {
                                        let _ = emit_json(&serde_json::json!({
                                            "event":"error", "message":error.to_string()
                                        }));
                                    }
                                }
                            }
                            let _ = task_done_sender.send(task_request_id);
                        });
                        transfer_tasks.insert(request_id, (control, task));
                    }
                    DaemonCommand::CancelTransfer { transfer_id } => {
                        if let Some((control, _)) = transfer_tasks.get(&transfer_id) {
                            control.cancel();
                            emit_json(&serde_json::json!({
                                "event":"transfer_cancelling",
                                "transfer_id":transfer_id
                            }))?;
                        }
                    }
                    DaemonCommand::CreateGroup { name, members } => {
                        if name.trim().is_empty() || name.chars().count() > 128 || members.is_empty() || members.len() > 15 {
                            emit_json(&serde_json::json!({"event":"error", "message":"a group needs a name and at least one contact"}))?;
                            continue;
                        }
                        let recipients = daemon_or_continue!(members.iter().map(|member| {
                            profile.contacts.iter()
                                .find(|contact| contact.name.eq_ignore_ascii_case(member))
                                .cloned()
                                .with_context(|| format!("contact not found: {member}"))
                        }).collect::<Result<Vec<_>>>());
                        if recipients.iter().any(|contact| contact.mls_key_package.is_empty()) {
                            emit_json(&serde_json::json!({"event":"error", "message":"all group members must connect once to exchange MLS key packages"}))?;
                            continue;
                        }
                        let group_id = ConversationId::random(&mut OsRng);
                        mls.create_group(group_id.as_bytes())?;
                        let packages = recipients.iter().map(|contact| contact.mls_key_package.as_slice()).collect::<Vec<_>>();
                        let welcome = mls.add_members(group_id.as_bytes(), &packages)?;
                        let mut identities = vec![profile.identity_id];
                        identities.extend(recipients.iter().map(|contact| contact.identity_id));
                        let created_at_ms = now_millis();
                        let history_since_ms = identities.iter().map(|identity| (*identity, created_at_ms)).collect();
                        let mut member_devices = vec![key.device_id()];
                        member_devices.extend(recipients.iter().map(|contact| contact.device_id));
                        let group = GroupProfile { id: group_id, name: name.trim().into(), owner: profile.identity_id, admins: vec![], members: identities, member_devices, history_since_ms };
                        store.create_conversation(group_id, profile.identity_id, &group.name, created_at_ms)?;
                        profile.groups.push(group.clone());
                        for recipient in &recipients {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, recipient, DirectPayload::GroupWelcome {
                                group: group.clone(), welcome: welcome.clone(),
                            }).await?;
                        }
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::GroupHistory { group_id } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        daemon_or_continue!(emit_group_history(&store, &profile, group_id));
                        if let Some(group) = profile.groups.iter().find(|group| group.id == group_id) {
                            emit_call_history(&store, &group.name)?;
                        }
                    }
                    DaemonCommand::GroupSend { group_id, message, reply_to } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        if message.is_empty() || message.len() > 64 * 1024 {
                            emit_json(&serde_json::json!({"event":"error", "message":"message must contain 1 byte to 64 KiB"}))?;
                            continue;
                        }
                        let reply_to = daemon_or_continue!(reply_to.as_deref()
                            .map(|value| value.parse::<EventId>().context("invalid reply message id"))
                            .transpose());
                        let (event, route) = publish_group_event(
                            &network, &key, &profile, &store, &mut mls, group_id,
                            EventBody::MessageCreate { content: MessageContent {
                                text: message.clone(), reply_to, attachment_ids: vec![],
                            }}
                        ).await?;
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        emit_json(&serde_json::json!({"event":"group_message", "group_id":group_id.to_string(), "message_id":event.event_id.to_string(), "author":profile.name, "body":message, "reply_to":reply_to.map(|id| id.to_string()), "outgoing":true, "delivery":route}))?;
                    }
                    DaemonCommand::GroupEditMessage { group_id, message_id, message } => {
                        let group_id = daemon_or_continue!(group_id.parse::<ConversationId>().context("invalid group id"));
                        let target = daemon_or_continue!(message_id.parse::<EventId>().context("invalid message id"));
                        if message.is_empty() || message.len() > 64 * 1024 {
                            emit_json(&serde_json::json!({"event":"error", "message":"message must contain 1 byte to 64 KiB"}))?;
                            continue;
                        }
                        let original = daemon_or_continue!(store.load_events(group_id)?.into_iter()
                            .find(|event| event.event_id == target && event.author_identity == profile.identity_id)
                            .and_then(|event| match event.body { EventBody::MessageCreate { content } => Some(content), _ => None })
                            .context("only the author can edit this message"));
                        let (event, _) = publish_group_event(
                            &network, &key, &profile, &store, &mut mls, group_id,
                            EventBody::MessageEdit { target, content: MessageContent {
                                text: message.clone(), reply_to: original.reply_to,
                                attachment_ids: original.attachment_ids,
                            }}
                        ).await?;
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        emit_json(&serde_json::json!({"event":"group_message_edited", "group_id":group_id.to_string(), "message_id":target.to_string(), "event_id":event.event_id.to_string(), "body":message}))?;
                    }
                    DaemonCommand::GroupDeleteMessage { group_id, message_id } => {
                        let group_id = daemon_or_continue!(group_id.parse::<ConversationId>().context("invalid group id"));
                        let target = daemon_or_continue!(message_id.parse::<EventId>().context("invalid message id"));
                        let owned = store.load_events(group_id)?.into_iter().any(|event| {
                            event.event_id == target && event.author_identity == profile.identity_id
                                && matches!(event.body, EventBody::MessageCreate { .. })
                        });
                        if !owned {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the author can delete this message"}))?;
                            continue;
                        }
                        publish_group_event(&network, &key, &profile, &store, &mut mls, group_id,
                            EventBody::MessageDelete { target }).await?;
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        emit_json(&serde_json::json!({"event":"group_message_deleted", "group_id":group_id.to_string(), "message_id":target.to_string()}))?;
                    }
                    DaemonCommand::GroupSendFile { group_id, path: file_path } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        match send_group_file_packets(
                            &network,
                            &key,
                            &profile,
                            &store,
                            &mut mls,
                            group_id,
                            &file_path,
                        ).await {
                            Ok((transfer_id, file_name, byte_len, delivery)) => {
                                persist_mls(&store, mls_snapshot_id, &mls)?;
                                emit_json(&serde_json::json!({
                                    "event":"group_file_sent", "group_id":group_id.to_string(),
                                    "transfer_id":hex::encode(transfer_id), "file_name":file_name,
                                    "byte_len":byte_len, "delivery":delivery, "outgoing":true
                                }))?;
                            }
                            Err(error) => emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?,
                        }
                    }
                    DaemonCommand::GroupAddMember { group_id, contact } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        let group_index = daemon_or_continue!(profile.groups.iter()
                            .position(|group| group.id == group_id).context("group not found"));
                        let can_manage = profile.groups[group_index].owner == profile.identity_id
                            || profile.groups[group_index].admins.contains(&profile.identity_id);
                        if !can_manage {
                            emit_json(&serde_json::json!({"event":"error", "message":"only group administrators can add members"}))?;
                            continue;
                        }
                        if profile.groups[group_index].members.len() >= 16 {
                            emit_json(&serde_json::json!({"event":"error", "message":"groups support at most 16 members"}))?;
                            continue;
                        }
                        let added = daemon_or_continue!(profile.contacts.iter()
                            .find(|item| item.name.eq_ignore_ascii_case(&contact)).cloned().context("contact not found"));
                        if added.mls_key_package.is_empty() {
                            emit_json(&serde_json::json!({"event":"error", "message":"contact has no MLS key package yet"}))?;
                            continue;
                        }
                        if profile.groups[group_index].members.contains(&added.identity_id) {
                            emit_json(&serde_json::json!({"event":"error", "message":"contact is already a member"}))?;
                            continue;
                        }
                        let old_group = profile.groups[group_index].clone();
                        let (welcome, commit) = mls.add_member_with_commit(group_id.as_bytes(), &added.mls_key_package)?;
                        profile.groups[group_index].members.push(added.identity_id);
                        profile.groups[group_index].member_devices.push(added.device_id);
                        profile.groups[group_index].history_since_ms.push((
                            added.identity_id,
                            next_group_logical_time(&store, group_id)?,
                        ));
                        let updated = profile.groups[group_index].clone();
                        for recipient in group_remote_contacts(&profile, &old_group, key.device_id()) {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, &recipient, DirectPayload::GroupCommit { group: updated.clone(), commit: commit.clone() }).await?;
                        }
                        deliver_payload_durable(&network, &key, &profile, &store, group_id, &added, DirectPayload::GroupWelcome { group: updated, welcome }).await?;
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::GroupRemoveMember { group_id, contact } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        let group_index = daemon_or_continue!(profile.groups.iter()
                            .position(|group| group.id == group_id).context("group not found"));
                        let is_owner = profile.groups[group_index].owner == profile.identity_id;
                        let can_manage = is_owner || profile.groups[group_index].admins.contains(&profile.identity_id);
                        if !can_manage {
                            emit_json(&serde_json::json!({"event":"error", "message":"only group administrators can remove members"}))?;
                            continue;
                        }
                        let removed = daemon_or_continue!(profile.contacts.iter()
                            .find(|item| item.name.eq_ignore_ascii_case(&contact)).cloned().context("contact not found"));
                        if !profile.groups[group_index].members.contains(&removed.identity_id) {
                            emit_json(&serde_json::json!({"event":"error", "message":"contact is not a member"}))?;
                            continue;
                        }
                        if removed.identity_id == profile.groups[group_index].owner
                            || (!is_owner && profile.groups[group_index].admins.contains(&removed.identity_id))
                        {
                            emit_json(&serde_json::json!({"event":"error", "message":"an administrator cannot remove the owner or another administrator"}))?;
                            continue;
                        }
                        let old_group = profile.groups[group_index].clone();
                        let removed_devices = profile.contacts.iter()
                            .filter(|item| item.identity_id == removed.identity_id)
                            .filter(|item| old_group.member_devices.contains(&item.device_id))
                            .map(|item| item.device_id)
                            .collect::<Vec<_>>();
                        let removed_credentials = removed_devices
                            .iter()
                            .map(|device| device.as_bytes().as_slice())
                            .collect::<Vec<_>>();
                        let commit = mls.remove_members(group_id.as_bytes(), &removed_credentials)?;
                        profile.groups[group_index].members.retain(|identity| *identity != removed.identity_id);
                        profile.groups[group_index].member_devices
                            .retain(|device| !removed_devices.contains(device));
                        profile.groups[group_index].history_since_ms.retain(|(identity, _)| *identity != removed.identity_id);
                        let updated = profile.groups[group_index].clone();
                        for recipient in group_remote_contacts(&profile, &old_group, key.device_id()) {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, &recipient, DirectPayload::GroupCommit { group: updated.clone(), commit: commit.clone() }).await?;
                        }
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::GroupSetAdmin { group_id, contact, admin } => {
                        let group_id = daemon_or_continue!(group_id.parse::<ConversationId>().context("invalid group id"));
                        let index = daemon_or_continue!(profile.groups.iter().position(|group| group.id == group_id).context("group not found"));
                        if profile.groups[index].owner != profile.identity_id {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the owner can manage administrators"}))?;
                            continue;
                        }
                        let identity = daemon_or_continue!(profile.contacts.iter()
                            .find(|item| item.name.eq_ignore_ascii_case(&contact))
                            .map(|item| item.identity_id).context("contact not found"));
                        if !profile.groups[index].members.contains(&identity) || identity == profile.groups[index].owner {
                            emit_json(&serde_json::json!({"event":"error", "message":"administrator must be a regular group member"}))?;
                            continue;
                        }
                        if admin && !profile.groups[index].admins.contains(&identity) {
                            profile.groups[index].admins.push(identity);
                        } else if !admin {
                            profile.groups[index].admins.retain(|candidate| *candidate != identity);
                        }
                        let updated = profile.groups[index].clone();
                        for recipient in group_remote_contacts(&profile, &updated, key.device_id()) {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, &recipient,
                                DirectPayload::GroupProfileUpdate { group: updated.clone() }).await?;
                        }
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::GroupTransferOwnership { group_id, contact } => {
                        let group_id = daemon_or_continue!(group_id.parse::<ConversationId>().context("invalid group id"));
                        let index = daemon_or_continue!(profile.groups.iter().position(|group| group.id == group_id).context("group not found"));
                        if profile.groups[index].owner != profile.identity_id {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the owner can transfer ownership"}))?;
                            continue;
                        }
                        let identity = daemon_or_continue!(profile.contacts.iter()
                            .find(|item| item.name.eq_ignore_ascii_case(&contact))
                            .map(|item| item.identity_id).context("contact not found"));
                        if !profile.groups[index].members.contains(&identity) {
                            emit_json(&serde_json::json!({"event":"error", "message":"new owner must be a group member"}))?;
                            continue;
                        }
                        profile.groups[index].admins.retain(|candidate| *candidate != identity);
                        profile.groups[index].owner = identity;
                        let updated = profile.groups[index].clone();
                        for recipient in group_remote_contacts(&profile, &updated, key.device_id()) {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, &recipient,
                                DirectPayload::GroupProfileUpdate { group: updated.clone() }).await?;
                        }
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::GroupDissolve { group_id } => {
                        let group_id = daemon_or_continue!(group_id.parse::<ConversationId>().context("invalid group id"));
                        let index = daemon_or_continue!(profile.groups.iter().position(|group| group.id == group_id).context("group not found"));
                        if profile.groups[index].owner != profile.identity_id {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the owner can dissolve the group"}))?;
                            continue;
                        }
                        let group = profile.groups[index].clone();
                        for recipient in group_remote_contacts(&profile, &group, key.device_id()) {
                            deliver_payload_durable(&network, &key, &profile, &store, group_id, &recipient,
                                DirectPayload::GroupDissolve { group_id }).await?;
                        }
                        profile.groups.remove(index);
                        save_profile(path, &profile)?;
                        emit_groups(&profile)?;
                    }
                    DaemonCommand::StartCall { contact, ring } => {
                        if active_call.is_some() {
                            emit_json(&serde_json::json!({"event":"error", "message":"hold the current call before starting another"}))?;
                            continue;
                        }
                        let recipients = daemon_or_continue!(resolve_call_recipients(&profile, &contact));
                        let call_id = CallId::random(&mut OsRng);
                        let started_at_unix = OffsetDateTime::now_utc().unix_timestamp();
                        let selected = recipients
                            .iter()
                            .map(|recipient| recipient.identity_id)
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        let signal = CallSignal::Invite {
                            call_id,
                            selected,
                            ring,
                        };
                        let participants = call_participants_json(&recipients);
                        for recipient in &recipients {
                            daemon_or_continue!(network.send_call_signal(&recipient.address, &signal).await);
                        }
                        store.save_call_event(&CallEventRecord {
                            call_id: *call_id.as_bytes(), conversation_key: contact.clone(),
                            direction: "outgoing".into(), outcome: if ring { "ringing".into() } else { "joined".into() },
                            started_at_unix, duration_ms: 0,
                        })?;
                        active_call = Some(ActiveCall {
                            id: call_id, label: contact.clone(), recipients, started_at_unix,
                            connected_at: (!ring).then(std::time::Instant::now),
                        });
                        emit_json(&serde_json::json!({"event":"call_started", "call_id":call_id.to_string(), "contact":contact, "ring":ring, "participants":participants}))?;
                    }
                    DaemonCommand::JoinCall { contact, call_id } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        let recipients = daemon_or_continue!(resolve_call_recipients(&profile, &contact));
                        let participants = call_participants_json(&recipients);
                        if active_call.as_ref().is_some_and(|call| call.id != call_id) {
                            if held_call.is_some() {
                                emit_json(&serde_json::json!({"event":"error", "message":"only one held call is supported"}))?;
                                continue;
                            }
                            let previous = active_call.take().expect("checked active call");
                            for recipient in &previous.recipients {
                                network.send_call_signal(&recipient.address, &CallSignal::Hold { call_id: previous.id }).await.ok();
                            }
                            for (_, task) in media_tasks.drain(..) { task.abort(); }
                            for kind in [MediaKind::Voice, MediaKind::Camera, MediaKind::Screen, MediaKind::SystemAudio] {
                                media.unpublish(kind).await.ok();
                                media.stop_receiving(kind).await.ok();
                            }
                            emit_json(&serde_json::json!({"event":"call_held", "call_id":previous.id.to_string(), "contact":previous.label}))?;
                            held_call = Some(previous);
                        }
                        for recipient in &recipients {
                            daemon_or_continue!(network.send_call_signal(&recipient.address, &CallSignal::Join { call_id }).await);
                        }
                        let started_at_unix = OffsetDateTime::now_utc().unix_timestamp();
                        store.save_call_event(&CallEventRecord {
                            call_id: *call_id.as_bytes(), conversation_key: contact.clone(),
                            direction: "incoming".into(), outcome: "answered".into(),
                            started_at_unix, duration_ms: 0,
                        })?;
                        active_call = Some(ActiveCall {
                            id: call_id, label: contact.clone(), recipients, started_at_unix,
                            connected_at: Some(std::time::Instant::now()),
                        });
                        emit_json(&serde_json::json!({"event":"call_joined", "call_id":call_id.to_string(), "contact":contact, "participants":participants}))?;
                    }
                    DaemonCommand::RejectCall { contact, call_id, missed } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        let recipients = daemon_or_continue!(resolve_call_recipients(&profile, &contact));
                        for recipient in &recipients {
                            network.send_call_signal(&recipient.address, &CallSignal::Reject { call_id, missed }).await.ok();
                        }
                        store.save_call_event(&CallEventRecord {
                            call_id: *call_id.as_bytes(), conversation_key: contact.clone(),
                            direction: "incoming".into(), outcome: if missed { "missed".into() } else { "rejected".into() },
                            started_at_unix: OffsetDateTime::now_utc().unix_timestamp(), duration_ms: 0,
                        })?;
                        emit_json(&serde_json::json!({"event":"call_rejected", "call_id":call_id.to_string(), "contact":contact, "outcome":if missed { "missed" } else { "rejected" }}))?;
                    }
                    DaemonCommand::HoldCall { call_id } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        if active_call.as_ref().is_none_or(|call| call.id != call_id) {
                            emit_json(&serde_json::json!({"event":"error", "message":"active call not found"}))?;
                            continue;
                        }
                        let call = active_call.take().expect("active call was checked");
                        for recipient in &call.recipients {
                            network.send_call_signal(&recipient.address, &CallSignal::Hold { call_id }).await.ok();
                        }
                        held_call = Some(call);
                        emit_json(&serde_json::json!({"event":"call_held", "call_id":call_id.to_string()}))?;
                    }
                    DaemonCommand::ResumeCall { call_id } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        if active_call.is_some() {
                            emit_json(&serde_json::json!({"event":"error", "message":"another call is active"}))?;
                            continue;
                        }
                        let Some(call) = held_call.take().filter(|call| call.id == call_id) else {
                            emit_json(&serde_json::json!({"event":"error", "message":"held call not found"}))?;
                            continue;
                        };
                        for recipient in &call.recipients {
                            network.send_call_signal(&recipient.address, &CallSignal::Resume { call_id }).await.ok();
                        }
                        active_call = Some(call);
                        emit_json(&serde_json::json!({"event":"call_resumed", "call_id":call_id.to_string()}))?;
                    }
                    DaemonCommand::LeaveCall { contact, call_id, missed } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        let recipients = if let Some(call) = active_call.as_ref().filter(|call| call.id == call_id) {
                            call.recipients.clone()
                        } else {
                            daemon_or_continue!(resolve_call_recipients(&profile, &contact))
                        };
                        for recipient in &recipients {
                            daemon_or_continue!(network.send_call_signal(&recipient.address, &CallSignal::Leave { call_id }).await);
                        }
                        for (_, task) in media_tasks.drain(..) {
                            task.abort();
                        }
                        for kind in [MediaKind::Voice, MediaKind::Camera, MediaKind::Screen, MediaKind::SystemAudio] {
                            media.unpublish(kind).await.ok();
                            media.stop_receiving(kind).await.ok();
                        }
                        if active_call.as_ref().is_some_and(|call| call.id == call_id) {
                            let ended = active_call.take().expect("active call was checked");
                            store.save_call_event(&CallEventRecord {
                                call_id: *call_id.as_bytes(), conversation_key: ended.label,
                                direction: "local".into(), outcome: if missed { "missed".into() } else { "ended".into() },
                                started_at_unix: ended.started_at_unix,
                                duration_ms: ended.connected_at.map_or(0, |started| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
                            })?;
                        } else if held_call.as_ref().is_some_and(|call| call.id == call_id) {
                            held_call = None;
                        }
                        emit_json(&serde_json::json!({"event":"call_left", "call_id":call_id.to_string(), "contact":contact, "outcome":if missed { "missed" } else { "ended" }}))?;
                        if active_call.is_none() && let Some(call) = held_call.take() {
                            for recipient in &call.recipients {
                                network.send_call_signal(&recipient.address, &CallSignal::Resume { call_id: call.id }).await.ok();
                            }
                            emit_json(&serde_json::json!({"event":"call_resumed", "call_id":call.id.to_string(), "contact":call.label}))?;
                            active_call = Some(call);
                        }
                    }
                    DaemonCommand::SetMedia { contact, call_id, kind, enabled, profile: requested } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        let Some(call) = active_call.as_ref().filter(|call| call.id == call_id && call.label.eq_ignore_ascii_case(&contact)) else {
                            emit_json(&serde_json::json!({"event":"error", "message":"media requires an active joined call"}))?;
                            continue;
                        };
                        let recipients = call.recipients.clone();
                        if enabled {
                            let quality = requested.unwrap_or_else(|| default_media_profile(kind));
                            match media.publish(kind, quality.clone()).await {
                                Ok(()) => {
                                    for recipient in &recipients {
                                        daemon_or_continue!(network.send_call_signal(&recipient.address, &CallSignal::Media {
                                            call_id, signal: MediaSignal::Publish { kind, profile: quality.clone() },
                                        }).await);
                                    }
                                    if let Some(index) = media_tasks.iter().position(|(active, _)| *active == kind) {
                                        let (_, task) = media_tasks.swap_remove(index);
                                        task.abort();
                                    }
                                    let mut sessions = Vec::new();
                                    for recipient in &recipients {
                                        sessions.push(daemon_or_continue!(network.connect_media(&recipient.address).await));
                                    }
                                    let media_for_task = Arc::clone(&media);
                                    let sender = key.device_id();
                                    let task = tokio::spawn(async move {
                                        let started = std::time::Instant::now();
                                        let mut sequence = 0_u64;
                                        loop {
                                            match media_for_task.next_rtp_packet(kind).await {
                                                Ok(Some(payload)) => {
                                                    let timestamp = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
                                                    let packet = MediaDatagram::new(call_id, sender, kind, sequence, timestamp, false, payload);
                                                    let Ok(bytes) = packet.to_wire() else { break; };
                                                    let mut delivered = false;
                                                    for session in &sessions {
                                                        delivered |= session.send(bytes.clone()).await.is_ok();
                                                    }
                                                    if !delivered { break; }
                                                    sequence = sequence.saturating_add(1);
                                                }
                                                Ok(None) => {}
                                                Err(_) => break,
                                            }
                                        }
                                    });
                                    media_tasks.push((kind, task));
                                    emit_json(&serde_json::json!({"event":"media_changed", "kind":kind, "enabled":true}))?;
                                }
                                Err(error) => emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?,
                            }
                        } else {
                            if let Some(index) = media_tasks.iter().position(|(active, _)| *active == kind) {
                                let (_, task) = media_tasks.swap_remove(index);
                                task.abort();
                            }
                            media.unpublish(kind).await?;
                            for recipient in &recipients {
                                daemon_or_continue!(network.send_call_signal(&recipient.address, &CallSignal::Media {
                                    call_id, signal: MediaSignal::Unpublish { kind },
                                }).await);
                            }
                            emit_json(&serde_json::json!({"event":"media_changed", "kind":kind, "enabled":false}))?;
                        }
                    }
                    DaemonCommand::SetParticipantVolume { call_id, device_id, volume } => {
                        let call_id = daemon_or_continue!(
                            call_id.parse::<CallId>().context("invalid call id")
                        );
                        let device = daemon_or_continue!(
                            device_id.parse::<DeviceId>().context("invalid device id")
                        );
                        if active_call.as_ref().is_none_or(|call| {
                            call.id != call_id ||
                                !call.recipients.iter().any(|recipient| recipient.device_id == device)
                        }) {
                            emit_json(&serde_json::json!({
                                "event":"error", "message":"call participant not found"
                            }))?;
                            continue;
                        }
                        match media.set_receive_volume(device, volume).await {
                            Ok(()) => emit_json(&serde_json::json!({
                                "event":"participant_volume", "device_id":device,
                                "volume":volume
                            }))?,
                            Err(error) => emit_json(&serde_json::json!({
                                "event":"error", "message":error.to_string()
                            }))?,
                        }
                    }
                    DaemonCommand::Shutdown => break,
                }
            }
            completed = transfer_done_receiver.recv(), if !transfer_tasks.is_empty() => {
                if let Some(completed) = completed
                    && let Some((_, task)) = transfer_tasks.remove(&completed)
                    && let Err(error) = task.await
                {
                    tracing::warn!(%error, transfer_id = %completed, "file transfer task failed");
                }
            }
            incoming = network.receive() => {
                let incoming = incoming?;
                match decrypt_incoming(&profile, &incoming.bytes) {
                    Ok((packet, shared_secret, was_pending)) => {
                        if was_pending {
                            upsert_contact(&mut profile, Contact {
                                name: packet.sender_name.clone(), identity_id: packet.sender_identity,
                                avatar: None,
                                device_id: packet.sender_device, public_key: packet.sender_public_key,
                                address: packet.return_address.clone(), mailbox_urls: packet.mailbox_urls.clone(),
                                shared_secret, verified: true, manually_verified: false,
                                mls_key_package: packet.mls_key_package.clone(),
                                identity_events: packet.identity_events.clone(),
                                blocked: false, removed: false, hide_presence: false,
                            });
                            profile.pending_invites.retain(|pending| pending.shared_secret != shared_secret);
                            save_profile(path, &profile)?;
                            emit_contacts(&profile)?;
                        }
                        let device_changed = refresh_contact_device(&mut profile, &packet, shared_secret)?;
                        let key_changed = update_contact_key_package(&mut profile, &packet);
                        let group_device_removed = remove_revoked_devices_from_owned_groups(
                            &network, &key, &store, &mut profile, &mut mls,
                            packet.sender_identity, &packet.identity_events,
                        ).await?;
                        let group_device_added = add_authorized_device_to_owned_groups(
                            &network, &key, &store, &mut profile, &mut mls, &packet,
                        ).await?;
                        let groups_changed = group_device_removed || group_device_added;
                        if device_changed || key_changed || groups_changed {
                            save_profile(path, &profile)?;
                            if device_changed { emit_contacts(&profile)?; }
                            if groups_changed {
                                persist_mls(&store, mls_snapshot_id, &mls)?;
                                emit_groups(&profile)?;
                            }
                        }
                        if matches!(packet.payload, DirectPayload::DeviceHello)
                            && packet.sender_identity == profile.identity_id
                            && let Some(recipient) = profile.contacts.iter()
                                .find(|contact| contact.device_id == packet.sender_device)
                                .cloned()
                        {
                            send_device_history(&network, &key, &profile, &store, &recipient).await?;
                        }
                        if sender_is_blocked(&profile, &packet) && !is_group_payload(&packet.payload) {
                            continue;
                        }
                        if handle_group_payload(
                            &network, &key, &store, path, &mut profile, &mut mls, &packet,
                            &mut incoming_files, "direct",
                        ).await? {
                            persist_mls(&store, mls_snapshot_id, &mls)?;
                        } else {
                            handle_incoming_payload(&store, path, profile.identity_id, &packet, shared_secret, &mut incoming_files, "direct")?;
                            acknowledge_incoming_message(&network, &key, &profile, &store, &packet).await?;
                        }
                    }
                    Err(error) => emit_json(&serde_json::json!({"event":"rejected", "message":error.to_string()}))?,
                }
            }
            _ = mailbox_tick.tick(), if profile.mailbox_url.is_some() => {
                match drain_mailbox(&profile).await {
                    Ok(messages) => for bytes in messages {
                        match decrypt_incoming(&profile, &bytes) {
                            Ok((packet, shared_secret, was_pending)) => {
                                if was_pending {
                                    upsert_contact(&mut profile, Contact {
                                        name: packet.sender_name.clone(), identity_id: packet.sender_identity,
                                        avatar: None,
                                        device_id: packet.sender_device, public_key: packet.sender_public_key,
                                        address: packet.return_address.clone(), mailbox_urls: packet.mailbox_urls.clone(),
                                        shared_secret, verified: true, manually_verified: false,
                                        mls_key_package: packet.mls_key_package.clone(),
                                        identity_events: packet.identity_events.clone(),
                                        blocked: false, removed: false, hide_presence: false,
                                    });
                                    profile.pending_invites.retain(|pending| pending.shared_secret != shared_secret);
                                    save_profile(path, &profile)?;
                                    emit_contacts(&profile)?;
                                }
                                let device_changed = refresh_contact_device(&mut profile, &packet, shared_secret)?;
                                let key_changed = update_contact_key_package(&mut profile, &packet);
                                let group_device_removed = remove_revoked_devices_from_owned_groups(
                                    &network, &key, &store, &mut profile, &mut mls,
                                    packet.sender_identity, &packet.identity_events,
                                ).await?;
                                let group_device_added = add_authorized_device_to_owned_groups(
                                    &network, &key, &store, &mut profile, &mut mls, &packet,
                                ).await?;
                                let groups_changed = group_device_removed || group_device_added;
                                if device_changed || key_changed || groups_changed {
                                    save_profile(path, &profile)?;
                                    if device_changed { emit_contacts(&profile)?; }
                                    if groups_changed {
                                        persist_mls(&store, mls_snapshot_id, &mls)?;
                                        emit_groups(&profile)?;
                                    }
                                }
                                if sender_is_blocked(&profile, &packet) && !is_group_payload(&packet.payload) {
                                    continue;
                                }
                                if handle_group_payload(
                                    &network, &key, &store, path, &mut profile, &mut mls, &packet,
                                    &mut incoming_files, "mailbox",
                                ).await? {
                                    persist_mls(&store, mls_snapshot_id, &mls)?;
                                } else {
                                    handle_incoming_payload(&store, path, profile.identity_id, &packet, shared_secret, &mut incoming_files, "mailbox")?;
                                    acknowledge_incoming_message(&network, &key, &profile, &store, &packet).await?;
                                }
                            }
                            Err(error) => emit_json(&serde_json::json!({"event":"rejected", "message":error.to_string()}))?,
                        }
                    },
                    Err(error) => tracing::warn!(%error, "mailbox poll failed"),
                }
            }
            _ = outbox_tick.tick() => {
                match flush_outbox(&network, &store, &profile).await {
                    Ok(count) if count > 0 => emit_json(&serde_json::json!({
                        "event":"outbox_delivered", "count":count
                    }))?,
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "outbox retry failed"),
                }
            }
            signal = network.receive_call_signal() => {
                let (remote_endpoint, signal) = signal?;
                let remote_contact = profile.contacts.iter()
                    .find(|contact| contact.address.endpoint_id == remote_endpoint);
                match signal {
                    CallSignal::Invite { call_id, selected, ring } => {
                        if remote_contact.is_some_and(|contact| contact.blocked || contact.removed) {
                            continue;
                        }
                        let group_name = remote_contact.and_then(|sender| profile.groups.iter().find(|group| {
                            group.members.contains(&profile.identity_id)
                                && group.members.contains(&sender.identity_id)
                                && selected.iter().all(|identity| group.members.contains(identity))
                                && group.members.len() > 2
                        })).map(|group| group.name.clone());
                        let target = group_name.or_else(|| remote_contact.map(|contact| contact.name.clone()));
                        if let Some(target) = &target {
                            store.save_call_event(&CallEventRecord {
                                call_id: *call_id.as_bytes(), conversation_key: target.clone(),
                                direction: "incoming".into(), outcome: if ring { "ringing".into() } else { "room".into() },
                                started_at_unix: OffsetDateTime::now_utc().unix_timestamp(), duration_ms: 0,
                            })?;
                        }
                        emit_json(&serde_json::json!({
                            "event":"call_invite", "call_id":call_id.to_string(), "selected":selected,
                            "ring":ring, "remote_endpoint":remote_endpoint, "contact":target
                        }))?;
                    }
                    CallSignal::Join { call_id } => {
                        if let Some(call) = active_call.as_mut().filter(|call| call.id == call_id) {
                            call.connected_at.get_or_insert_with(std::time::Instant::now);
                            store.save_call_event(&CallEventRecord {
                                call_id: *call_id.as_bytes(), conversation_key: call.label.clone(),
                                direction: "outgoing".into(), outcome: "answered".into(),
                                started_at_unix: call.started_at_unix, duration_ms: 0,
                            })?;
                        }
                        emit_json(&serde_json::json!({
                            "event":"call_connected", "call_id":call_id.to_string(),
                            "remote_endpoint":remote_endpoint,
                            "contact":remote_contact.map(|contact| contact.name.clone()),
                            "device_id":remote_contact.map(|contact| contact.device_id)
                        }))?;
                    }
                    CallSignal::Reject { call_id, missed } => {
                        if active_call.as_ref().is_some_and(|call| call.id == call_id) {
                            let call = active_call.take().expect("active call was checked");
                            store.save_call_event(&CallEventRecord {
                                call_id: *call_id.as_bytes(), conversation_key: call.label,
                                direction: "outgoing".into(), outcome: if missed { "missed".into() } else { "rejected".into() },
                                started_at_unix: call.started_at_unix, duration_ms: 0,
                            })?;
                        }
                        emit_json(&serde_json::json!({"event":"call_rejected", "call_id":call_id.to_string(), "outcome":if missed { "missed" } else { "rejected" }}))?;
                    }
                    CallSignal::Hold { call_id } => {
                        emit_json(&serde_json::json!({"event":"call_remote_held", "call_id":call_id.to_string()}))?;
                    }
                    CallSignal::Resume { call_id } => {
                        emit_json(&serde_json::json!({"event":"call_remote_resumed", "call_id":call_id.to_string()}))?;
                    }
                    CallSignal::Leave { call_id } => {
                        let mut ended = false;
                        if let Some(call) = active_call.as_mut().filter(|call| call.id == call_id) {
                            call.recipients.retain(|recipient| recipient.address.endpoint_id != remote_endpoint);
                            ended = call.recipients.is_empty();
                        }
                        if ended {
                            for (_, task) in media_tasks.drain(..) { task.abort(); }
                            for kind in [MediaKind::Voice, MediaKind::Camera, MediaKind::Screen, MediaKind::SystemAudio] {
                                media.unpublish(kind).await.ok();
                                media.stop_receiving(kind).await.ok();
                            }
                            active_call = None;
                            emit_json(&serde_json::json!({"event":"call_leave", "call_id":call_id.to_string(), "remote_endpoint":remote_endpoint}))?;
                        } else {
                            emit_json(&serde_json::json!({
                                "event":"call_participant_leave", "call_id":call_id.to_string(),
                                "remote_endpoint":remote_endpoint,
                                "device_id":remote_contact.map(|contact| contact.device_id)
                            }))?;
                        }
                    }
                    CallSignal::Media { call_id, signal } => {
                        if let MediaSignal::Unpublish { kind } = &signal {
                            media.stop_receiving(*kind).await.ok();
                        }
                        emit_json(&serde_json::json!({"event":"remote_media", "call_id":call_id.to_string(), "signal":signal, "remote_endpoint":remote_endpoint}))?;
                    }
                    other => emit_json(&serde_json::json!({"event":"call_signal", "signal":other, "remote_endpoint":remote_endpoint}))?,
                }
            }
            incoming = network.receive_media() => {
                let incoming = incoming?;
                let packet = MediaDatagram::from_wire(&incoming.bytes)?;
                let known_sender = profile.contacts.iter().any(|contact| {
                    !contact.blocked
                        && !contact.removed
                        && contact.address.endpoint_id == incoming.remote_endpoint_id
                        && contact.device_id == packet.sender
                });
                if packet.version != PROTOCOL_VERSION || !known_sender {
                    emit_json(&serde_json::json!({"event":"rejected", "message":"unauthorized media datagram"}))?;
                    continue;
                }
                if active_call.as_ref().is_none_or(|call| call.id != packet.call_id) {
                    continue;
                }
                if let Err(error) = media
                    .receive_rtp_packet(packet.sender, packet.kind, packet.payload)
                    .await
                {
                    emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                break;
            }
        }
    }
    for (control, _) in transfer_tasks.values() {
        control.cancel();
    }
    for (transfer_id, (_, task)) in transfer_tasks {
        if let Err(error) = task.await {
            tracing::warn!(%error, %transfer_id, "file transfer shutdown failed");
        }
    }
    network.shutdown().await?;
    Ok(())
}

async fn check_for_update() -> Result<serde_json::Value> {
    let release: serde_json::Value = reqwest::Client::new()
        .get("https://api.github.com/repos/Arzuparreta/pptalk/releases/latest")
        .header(reqwest::header::USER_AGENT, "pptalk-update-check")
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let latest = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let current = env!("CARGO_PKG_VERSION");
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    let wanted_suffix = if cfg!(target_os = "windows") {
        format!("windows-{architecture}.exe")
    } else {
        format!("linux-{architecture}.AppImage")
    };
    let url = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| name.ends_with(&wanted_suffix))
            })
        })
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let available = release_version(latest)
        .is_some_and(|latest| release_version(current).is_some_and(|current| latest > current))
        && !url.is_empty();
    Ok(serde_json::json!({
        "event":"update", "available":available, "current":current,
        "version":latest, "url":url
    }))
}

fn release_version(value: &str) -> Option<(u64, u64, u64)> {
    let stable = value
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut components = stable.split('.');
    let version = (
        components.next()?.parse().ok()?,
        components.next()?.parse().ok()?,
        components.next()?.parse().ok()?,
    );
    components.next().is_none().then_some(version)
}

fn default_media_profile(kind: MediaKind) -> QualityProfile {
    match kind {
        MediaKind::Voice | MediaKind::SystemAudio => QualityProfile {
            mode: QualityMode::Automatic,
            width: 1,
            height: 1,
            frames_per_second: 1,
            bitrate_kbps: 64,
            codec: Some("opus".into()),
        },
        MediaKind::Camera => QualityProfile {
            mode: QualityMode::Automatic,
            width: 1280,
            height: 720,
            frames_per_second: 30,
            bitrate_kbps: 2_500,
            codec: Some("h264".into()),
        },
        MediaKind::Screen => QualityProfile {
            mode: QualityMode::Automatic,
            width: 1920,
            height: 1080,
            frames_per_second: 30,
            bitrate_kbps: 5_000,
            codec: Some("h264".into()),
        },
    }
}

fn direct_message_body(payload: &DirectPayload) -> Option<&str> {
    match payload {
        DirectPayload::Message { body, .. } => Some(body),
        DirectPayload::DeviceHello
        | DirectPayload::DeviceHistory { .. }
        | DirectPayload::MessageEdit { .. }
        | DirectPayload::MessageDelete { .. }
        | DirectPayload::DeliveryReceipt { .. }
        | DirectPayload::FileOffer { .. }
        | DirectPayload::FileChunk { .. }
        | DirectPayload::GroupWelcome { .. }
        | DirectPayload::GroupMessage { .. }
        | DirectPayload::GroupCommit { .. }
        | DirectPayload::GroupProfileUpdate { .. }
        | DirectPayload::GroupDissolve { .. }
        | DirectPayload::GroupFileOffer { .. }
        | DirectPayload::GroupFileChunk { .. }
        | DirectPayload::GroupSyncRequest { .. }
        | DirectPayload::GroupSyncEvent { .. } => None,
    }
}

fn sender_is_blocked(profile: &Profile, packet: &ChatPacket) -> bool {
    profile
        .contacts
        .iter()
        .any(|contact| contact.identity_id == packet.sender_identity && contact.blocked)
}

fn is_group_payload(payload: &DirectPayload) -> bool {
    matches!(
        payload,
        DirectPayload::GroupWelcome { .. }
            | DirectPayload::GroupMessage { .. }
            | DirectPayload::GroupCommit { .. }
            | DirectPayload::GroupProfileUpdate { .. }
            | DirectPayload::GroupDissolve { .. }
            | DirectPayload::GroupFileOffer { .. }
            | DirectPayload::GroupFileChunk { .. }
            | DirectPayload::GroupSyncRequest { .. }
            | DirectPayload::GroupSyncEvent { .. }
    )
}

fn signed_direct_packet(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    payload: DirectPayload,
) -> Result<ChatPacket> {
    let mut packet = ChatPacket {
        version: PROTOCOL_VERSION,
        sender_name: profile.name.clone(),
        sender_avatar: profile.avatar.clone(),
        sender_identity: profile.identity_id,
        sender_device: key.device_id(),
        sender_public_key: key.public_key(),
        identity_events: profile.identity_events.clone(),
        return_address: network.local_address(),
        mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
        mls_key_package: profile.mls_key_package.clone(),
        sent_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        payload,
        signature: vec![],
    };
    packet.signature = key.sign_message(&packet.to_wire()?);
    Ok(packet)
}

fn encrypt_direct_packet(
    packet: &ChatPacket,
    secret: &[u8; 32],
    recipient_device: DeviceId,
) -> Result<([u8; 32], Vec<u8>)> {
    let route = routing_capability(secret, recipient_device);
    let encrypted = GroupSecret::from_bytes(*secret).encrypt(&packet.to_wire()?, &route)?;
    let envelope = TransportEnvelope::new(route, encrypted.to_wire()?, 1024);
    Ok((route, envelope.to_wire()?))
}

async fn deliver_payload(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    recipient: &Contact,
    payload: DirectPayload,
) -> Result<&'static str> {
    let packet = signed_direct_packet(network, key, profile, payload)?;
    deliver_signed_packet(network, recipient, &packet).await
}

async fn deliver_signed_packet(
    network: &PeerNetwork,
    recipient: &Contact,
    packet: &ChatPacket,
) -> Result<&'static str> {
    let (route, bytes) =
        encrypt_direct_packet(packet, &recipient.shared_secret, recipient.device_id)?;
    match network.send(&recipient.address, &bytes).await {
        Ok(()) => Ok("direct"),
        Err(direct_error) => {
            deposit_to_any_mailbox(&recipient.mailbox_urls, &route, &bytes)
                .await
                .with_context(|| format!("direct delivery failed: {direct_error}"))?;
            Ok("mailbox")
        }
    }
}

async fn deliver_payload_durable(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    store: &Store,
    conversation_id: ConversationId,
    recipient: &Contact,
    payload: DirectPayload,
) -> Result<&'static str> {
    let packet = signed_direct_packet(network, key, profile, payload)?;
    deliver_signed_packet_durable(network, store, conversation_id, recipient, &packet).await
}

async fn deliver_signed_packet_durable(
    network: &PeerNetwork,
    store: &Store,
    conversation_id: ConversationId,
    recipient: &Contact,
    packet: &ChatPacket,
) -> Result<&'static str> {
    let (route, bytes) =
        encrypt_direct_packet(packet, &recipient.shared_secret, recipient.device_id)?;
    if network.send(&recipient.address, &bytes).await.is_ok() {
        return Ok("direct");
    }
    if deposit_to_any_mailbox(&recipient.mailbox_urls, &route, &bytes)
        .await
        .is_ok()
    {
        return Ok("mailbox");
    }
    let event_id = EventId::from_bytes(*blake3::hash(&bytes).as_bytes());
    store.enqueue(
        conversation_id,
        event_id,
        recipient.device_id,
        &bytes,
        next_outbox_time(),
    )?;
    Ok("queued")
}

async fn flush_outbox(network: &PeerNetwork, store: &Store, profile: &Profile) -> Result<usize> {
    let mut delivered = 0;
    for item in store.due_outbox(now_millis(), 128)? {
        let Some(recipient) = profile
            .contacts
            .iter()
            .find(|contact| contact.device_id == item.recipient)
        else {
            store.acknowledge(item.event_id, item.recipient)?;
            continue;
        };
        let route = routing_capability(&recipient.shared_secret, recipient.device_id);
        let sent = network
            .send(&recipient.address, &item.envelope)
            .await
            .is_ok()
            || deposit_to_any_mailbox(&recipient.mailbox_urls, &route, &item.envelope)
                .await
                .is_ok();
        if sent {
            store.acknowledge(item.event_id, item.recipient)?;
            delivered += 1;
        } else {
            let exponent = item.attempts.min(8);
            let delay = 2_000_i64.saturating_mul(1_i64 << exponent).min(300_000);
            store.defer_outbox(
                item.event_id,
                item.recipient,
                now_millis().saturating_add(delay),
            )?;
        }
    }
    Ok(delivered)
}

fn direct_conversation_id(local: IdentityId, remote: IdentityId) -> ConversationId {
    let (first, second) = if local <= remote {
        (local, remote)
    } else {
        (remote, local)
    };
    let mut material = Vec::with_capacity(81);
    material.extend_from_slice(b"pptalk-direct-v1");
    material.extend_from_slice(first.as_bytes());
    material.extend_from_slice(second.as_bytes());
    ConversationId::from_bytes(*blake3::hash(&material).as_bytes())
}

fn persist_mls(store: &Store, snapshot_id: ConversationId, mls: &MlsClient) -> Result<()> {
    store.save_mls_state(snapshot_id, &mls.snapshot()?, now_millis())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_group_payload(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    store: &Store,
    profile_path: &Path,
    profile: &mut Profile,
    mls: &mut MlsClient,
    packet: &ChatPacket,
    incoming_files: &mut std::collections::BTreeMap<[u8; 32], IncomingFile>,
    delivery: &str,
) -> Result<bool> {
    match &packet.payload {
        DirectPayload::GroupWelcome { group, welcome } => {
            if group.owner != packet.sender_identity
                || !group.members.contains(&profile.identity_id)
                || !group.members.contains(&group.owner)
            {
                bail!("invalid group welcome membership");
            }
            if profile.groups.iter().any(|known| known.id == group.id) {
                return Ok(true);
            }
            mls.join_group(group.id.as_bytes(), welcome)?;
            store.create_conversation(group.id, group.owner, &group.name, now_millis())?;
            profile.groups.push(group.clone());
            save_profile(profile_path, profile)?;
            emit_groups(profile)?;
            let frontier = conversation_frontier(store, group.id)?;
            if let Some(owner) = profile.contacts.iter().find(|contact| {
                contact.identity_id == packet.sender_identity
                    && contact.device_id == packet.sender_device
            }) {
                deliver_payload_durable(
                    network,
                    key,
                    profile,
                    store,
                    group.id,
                    owner,
                    DirectPayload::GroupSyncRequest {
                        group_id: group.id,
                        frontier,
                    },
                )
                .await?;
            }
            Ok(true)
        }
        DirectPayload::GroupMessage {
            group_id,
            ciphertext,
        } => {
            if sender_is_blocked(profile, packet) {
                return Ok(true);
            }
            let group = profile
                .groups
                .iter()
                .find(|group| group.id == *group_id)
                .context("message for unknown group")?;
            if !group.members.contains(&packet.sender_identity) {
                bail!("group sender is not a member");
            }
            let plaintext = mls.decrypt(group_id.as_bytes(), ciphertext)?;
            let event = ConversationEvent::from_wire(&plaintext)?;
            if event.conversation_id != *group_id || event.author_identity != packet.sender_identity
            {
                bail!("group event author mismatch");
            }
            if let EventBody::MessageEdit { target, .. } | EventBody::MessageDelete { target } =
                &event.body
            {
                let valid_target = store.load_events(*group_id)?.into_iter().any(|candidate| {
                    candidate.event_id == *target
                        && candidate.author_identity == packet.sender_identity
                        && matches!(candidate.body, EventBody::MessageCreate { .. })
                });
                if !valid_target {
                    bail!("group message mutation is not authored by the original sender");
                }
            }
            let inserted = store.save_event(&event)?;
            if inserted {
                match event.body {
                    EventBody::MessageCreate { content } => emit_json(&serde_json::json!({
                        "event":"group_message", "group_id":group_id.to_string(),
                        "message_id":event.event_id.to_string(), "author":packet.sender_name,
                        "body":content.text, "reply_to":content.reply_to.map(|id| id.to_string()),
                        "outgoing":false
                    }))?,
                    EventBody::MessageEdit { target, content } => emit_json(&serde_json::json!({
                        "event":"group_message_edited", "group_id":group_id.to_string(),
                        "message_id":target.to_string(), "body":content.text
                    }))?,
                    EventBody::MessageDelete { target } => emit_json(&serde_json::json!({
                        "event":"group_message_deleted", "group_id":group_id.to_string(),
                        "message_id":target.to_string()
                    }))?,
                    _ => {}
                }
            }
            Ok(true)
        }
        DirectPayload::GroupCommit { group, commit } => {
            let Some(index) = profile.groups.iter().position(|known| known.id == group.id) else {
                bail!("commit for unknown group");
            };
            let previous = &profile.groups[index];
            let authorized = previous.owner == packet.sender_identity
                || previous.admins.contains(&packet.sender_identity);
            if !authorized || group.owner != previous.owner {
                bail!("only group administrators can commit membership changes");
            }
            match mls.decrypt(group.id.as_bytes(), commit) {
                Err(MlsError::ControlMessage) => {}
                Ok(_) => bail!("membership commit decoded as application data"),
                Err(error) => return Err(error.into()),
            }
            if group.members.contains(&profile.identity_id) {
                profile.groups[index] = group.clone();
            } else {
                profile.groups.remove(index);
            }
            save_profile(profile_path, profile)?;
            emit_groups(profile)?;
            Ok(true)
        }
        DirectPayload::GroupProfileUpdate { group } => {
            let Some(index) = profile.groups.iter().position(|known| known.id == group.id) else {
                bail!("profile update for unknown group");
            };
            let previous = &profile.groups[index];
            if previous.owner != packet.sender_identity
                || previous.members != group.members
                || previous.member_devices != group.member_devices
                || !group.members.contains(&group.owner)
                || group
                    .admins
                    .iter()
                    .any(|admin| !group.members.contains(admin) || *admin == group.owner)
            {
                bail!("invalid group profile update");
            }
            profile.groups[index] = group.clone();
            save_profile(profile_path, profile)?;
            emit_groups(profile)?;
            Ok(true)
        }
        DirectPayload::GroupDissolve { group_id } => {
            let Some(index) = profile
                .groups
                .iter()
                .position(|known| known.id == *group_id)
            else {
                return Ok(true);
            };
            if profile.groups[index].owner != packet.sender_identity {
                bail!("only the group owner can dissolve the group");
            }
            profile.groups.remove(index);
            save_profile(profile_path, profile)?;
            emit_groups(profile)?;
            Ok(true)
        }
        DirectPayload::GroupFileOffer {
            group_id,
            transfer_id,
            encrypted_secret,
            manifest,
            event,
        } => {
            if sender_is_blocked(profile, packet) {
                return Ok(true);
            }
            validate_group_file_sender(profile, packet, *group_id)?;
            if manifest.ciphertext_hash != *transfer_id || manifest.chunk_hashes.len() > 65_536 {
                bail!("invalid group attachment manifest");
            }
            if event.conversation_id != *group_id
                || event.author_identity != packet.sender_identity
                || !event_references_attachment(event, transfer_id)
            {
                bail!("group attachment event does not match its sender or transfer");
            }
            let plaintext_secret = mls.decrypt(group_id.as_bytes(), encrypted_secret)?;
            let secret: [u8; 32] = plaintext_secret
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid group attachment key length"))?;
            let incoming = incoming_files.entry(*transfer_id).or_default();
            incoming.secret = Some(secret);
            incoming.manifest = Some(manifest.clone());
            incoming.group_id = Some(*group_id);
            incoming.group_event = Some(event.as_ref().clone());
            emit_json(&serde_json::json!({
                "event":"group_file_progress", "group_id":group_id.to_string(),
                "from":packet.sender_name, "file_name":manifest.file_name,
                "received_chunks":incoming.chunks.len(), "total_chunks":manifest.chunk_hashes.len(),
                "delivery":delivery
            }))?;
            finish_incoming_group_file(store, profile_path, packet, *transfer_id, incoming_files)?;
            Ok(true)
        }
        DirectPayload::GroupFileChunk {
            group_id,
            transfer_id,
            index,
            ciphertext,
        } => {
            if sender_is_blocked(profile, packet) {
                return Ok(true);
            }
            validate_group_file_sender(profile, packet, *group_id)?;
            if ciphertext.len() > 4 * 1024 * 1024 {
                bail!("group attachment chunk exceeds protocol limit");
            }
            let incoming = incoming_files.entry(*transfer_id).or_default();
            if incoming.group_id.is_some_and(|known| known != *group_id) {
                bail!("group attachment changed conversation");
            }
            incoming.group_id = Some(*group_id);
            incoming
                .chunks
                .entry(*index)
                .or_insert_with(|| ciphertext.clone());
            finish_incoming_group_file(store, profile_path, packet, *transfer_id, incoming_files)?;
            Ok(true)
        }
        DirectPayload::GroupSyncRequest { group_id, frontier } => {
            if sender_is_blocked(profile, packet) {
                return Ok(true);
            }
            let group = profile
                .groups
                .iter()
                .find(|group| group.id == *group_id)
                .cloned()
                .context("sync request for unknown group")?;
            if !group.members.contains(&packet.sender_identity) {
                bail!("group sync requester is not a member");
            }
            let history_floor = group
                .history_since_ms
                .iter()
                .find(|(identity, _)| *identity == packet.sender_identity)
                .map_or(i64::MAX, |(_, floor)| *floor);
            let recipient = profile
                .contacts
                .iter()
                .find(|contact| {
                    contact.identity_id == packet.sender_identity
                        && contact.device_id == packet.sender_device
                })
                .cloned()
                .context("group sync requester device is not a contact")?;
            for event in store
                .events_after(*group_id, frontier)?
                .into_iter()
                .filter(|event| {
                    event.logical_time_ms >= history_floor
                        && event.author_identity == profile.identity_id
                })
            {
                let ciphertext = mls.encrypt(group_id.as_bytes(), &event.to_wire()?)?;
                deliver_payload_durable(
                    network,
                    key,
                    profile,
                    store,
                    *group_id,
                    &recipient,
                    DirectPayload::GroupSyncEvent {
                        group_id: *group_id,
                        ciphertext,
                    },
                )
                .await?;
            }
            Ok(true)
        }
        DirectPayload::GroupSyncEvent {
            group_id,
            ciphertext,
        } => {
            if sender_is_blocked(profile, packet) {
                return Ok(true);
            }
            let group = profile
                .groups
                .iter()
                .find(|group| group.id == *group_id)
                .context("sync event for unknown group")?;
            if !group.members.contains(&packet.sender_identity) {
                bail!("group sync sender is not a member");
            }
            let plaintext = mls.decrypt(group_id.as_bytes(), ciphertext)?;
            let event = ConversationEvent::from_wire(&plaintext)?;
            if event.conversation_id != *group_id
                || event.author_identity != packet.sender_identity
                || event.logical_time_ms
                    < group
                        .history_since_ms
                        .iter()
                        .find(|(identity, _)| *identity == profile.identity_id)
                        .map_or(i64::MAX, |(_, floor)| *floor)
            {
                bail!("group sync event is outside the authorized history window");
            }
            if let EventBody::MessageEdit { target, .. } | EventBody::MessageDelete { target } =
                &event.body
            {
                let valid_target = store.load_events(*group_id)?.into_iter().any(|candidate| {
                    candidate.event_id == *target
                        && candidate.author_identity == packet.sender_identity
                        && matches!(candidate.body, EventBody::MessageCreate { .. })
                });
                if !valid_target {
                    bail!("synced group mutation is not authored by the original sender");
                }
            }
            if store.save_event(&event)? {
                match event.body {
                    EventBody::MessageCreate { content } => {
                        let outgoing = event.author_identity == profile.identity_id;
                        let author = if outgoing {
                            profile.name.clone()
                        } else {
                            profile
                                .contacts
                                .iter()
                                .find(|contact| contact.identity_id == event.author_identity)
                                .map_or_else(
                                    || event.author_identity.short(),
                                    |contact| contact.name.clone(),
                                )
                        };
                        emit_json(&serde_json::json!({
                            "event":"group_message", "group_id":group_id.to_string(),
                            "message_id":event.event_id.to_string(), "author":author,
                            "body":content.text, "reply_to":content.reply_to.map(|id| id.to_string()),
                            "outgoing":outgoing, "synced":true
                        }))?;
                    }
                    EventBody::MessageEdit { target, content } => emit_json(&serde_json::json!({
                        "event":"group_message_edited", "group_id":group_id.to_string(),
                        "message_id":target.to_string(), "body":content.text, "synced":true
                    }))?,
                    EventBody::MessageDelete { target } => emit_json(&serde_json::json!({
                        "event":"group_message_deleted", "group_id":group_id.to_string(),
                        "message_id":target.to_string(), "synced":true
                    }))?,
                    _ => {}
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn conversation_frontier(store: &Store, group_id: ConversationId) -> Result<CausalFrontier> {
    Ok(store.load_events(group_id)?.into_iter().fold(
        CausalFrontier::new(),
        |mut frontier, event| {
            frontier
                .entry(event.author_device)
                .and_modify(|known| *known = (*known).max(event.device_sequence))
                .or_insert(event.device_sequence);
            frontier
        },
    ))
}

fn next_group_logical_time(store: &Store, group_id: ConversationId) -> Result<i64> {
    Ok(store
        .load_events(group_id)?
        .into_iter()
        .map(|event| event.logical_time_ms)
        .max()
        .map_or_else(now_millis, |latest| {
            now_millis().max(latest.saturating_add(1))
        }))
}

async fn publish_group_event(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    store: &Store,
    mls: &mut MlsClient,
    group_id: ConversationId,
    body: EventBody,
) -> Result<(ConversationEvent, &'static str)> {
    let group = profile
        .groups
        .iter()
        .find(|group| group.id == group_id)
        .cloned()
        .context("group not found")?;
    let frontier = store.load_events(group_id)?.into_iter().fold(
        CausalFrontier::new(),
        |mut frontier, event| {
            frontier
                .entry(event.author_device)
                .and_modify(|value| *value = (*value).max(event.device_sequence))
                .or_insert(event.device_sequence);
            frontier
        },
    );
    let mut builder =
        ConversationBuilder::new(group_id, profile.identity_id, key.device_id(), frontier);
    let event = builder.build(body, next_group_logical_time(store, group_id)?, &mut OsRng)?;
    let ciphertext = mls.encrypt(group_id.as_bytes(), &event.to_wire()?)?;
    store.save_event(&event)?;
    let mut route = "direct";
    for recipient in group_remote_contacts(profile, &group, key.device_id()) {
        let delivery = deliver_payload_durable(
            network,
            key,
            profile,
            store,
            group_id,
            &recipient,
            DirectPayload::GroupMessage {
                group_id,
                ciphertext: ciphertext.clone(),
            },
        )
        .await?;
        match delivery {
            "queued" => route = "queued",
            "mailbox" if route == "direct" => route = "mailbox",
            _ => {}
        }
    }
    Ok((event, route))
}

fn validate_group_file_sender(
    profile: &Profile,
    packet: &ChatPacket,
    group_id: ConversationId,
) -> Result<()> {
    let group = profile
        .groups
        .iter()
        .find(|group| group.id == group_id)
        .context("attachment for unknown group")?;
    if !group.members.contains(&packet.sender_identity) {
        bail!("group attachment sender is not a member");
    }
    Ok(())
}

fn event_references_attachment(event: &ConversationEvent, transfer_id: &[u8; 32]) -> bool {
    matches!(
        &event.body,
        EventBody::MessageCreate { content }
            if content.attachment_ids.iter().any(|attachment| attachment == transfer_id)
    )
}

fn finish_incoming_group_file(
    store: &Store,
    profile_path: &Path,
    packet: &ChatPacket,
    transfer_id: [u8; 32],
    incoming_files: &mut std::collections::BTreeMap<[u8; 32], IncomingFile>,
) -> Result<()> {
    let Some(incoming) = incoming_files.get(&transfer_id) else {
        return Ok(());
    };
    let (Some(secret), Some(manifest), Some(group_id), Some(event)) = (
        incoming.secret,
        incoming.manifest.clone(),
        incoming.group_id,
        incoming.group_event.clone(),
    ) else {
        return Ok(());
    };
    if incoming.chunks.len() != manifest.chunk_hashes.len() {
        return Ok(());
    }
    let mut chunks = Vec::with_capacity(manifest.chunk_hashes.len());
    for index in 0..manifest.chunk_hashes.len() {
        let index = u32::try_from(index).context("attachment index overflow")?;
        let Some(chunk) = incoming.chunks.get(&index) else {
            return Ok(());
        };
        chunks.push(chunk.clone());
    }
    let plaintext = decrypt_blob(
        &GroupSecret::from_bytes(secret),
        &EncryptedBlob {
            manifest: manifest.clone(),
            chunks,
        },
    )?;
    let safe_name = Path::new(&manifest.file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment.bin");
    let directory = profile_path.with_extension("files");
    std::fs::create_dir_all(&directory)?;
    let destination = directory.join(format!("{}-{safe_name}", &hex::encode(transfer_id)[..12]));
    if !destination.exists() {
        let temporary = destination.with_extension("part");
        std::fs::write(&temporary, plaintext)?;
        std::fs::rename(&temporary, &destination)?;
    }
    store.save_event(&event)?;
    incoming_files.remove(&transfer_id);
    emit_json(&serde_json::json!({
        "event":"group_file_received", "group_id":group_id.to_string(),
        "from":packet.sender_name, "file_name":manifest.file_name,
        "byte_len":manifest.byte_len, "path":destination,
        "sent_at":event.logical_time_ms / 1000, "outgoing":false
    }))?;
    Ok(())
}

async fn send_file_packets(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    recipient: &Contact,
    path: &Path,
    queued: &mut Vec<QueuedFilePacket>,
    progress: DirectTransferProgress<'_>,
) -> Result<([u8; 32], String, u64, &'static str)> {
    if progress.control.is_cancelled() {
        bail!("transfer cancelled");
    }
    let metadata = std::fs::metadata(path).with_context(|| format!("read {}", path.display()))?;
    if !metadata.is_file() {
        bail!("attachment must be a regular file");
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        bail!("attachment exceeds the 512 MiB client limit");
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("attachment file name is not valid UTF-8")?
        .to_owned();
    let plaintext = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let blob = encrypt_blob(
        &GroupSecret::from_bytes(recipient.shared_secret),
        &plaintext,
        file_name.clone(),
        "application/octet-stream",
        256 * 1024,
    )?;
    let transfer_id = blob.manifest.ciphertext_hash;
    let total_chunks = blob.chunks.len();
    let overall_total = total_chunks.saturating_mul(progress.device_count);
    emit_json(&serde_json::json!({
        "event":"transfer_progress", "transfer_id":progress.id,
        "file_name":file_name,
        "sent_chunks":progress.device_index.saturating_mul(total_chunks),
        "total_chunks":overall_total, "cancelable":true
    }))?;
    if progress.control.is_cancelled() {
        bail!("transfer cancelled");
    }
    let offer = signed_direct_packet(
        network,
        key,
        profile,
        DirectPayload::FileOffer {
            transfer_id,
            manifest: blob.manifest.clone(),
        },
    )?;
    let conversation_id = direct_conversation_id(profile.identity_id, recipient.identity_id);
    let mut delivery = tokio::select! {
        biased;
        () = progress.control.cancelled() => bail!("transfer cancelled"),
        delivery = deliver_file_packet(
            network, conversation_id, recipient, &offer, queued
        ) => delivery?,
    };
    for (index, ciphertext) in blob.chunks.into_iter().enumerate() {
        if progress.control.is_cancelled() {
            bail!("transfer cancelled");
        }
        let packet = signed_direct_packet(
            network,
            key,
            profile,
            DirectPayload::FileChunk {
                transfer_id,
                index: u32::try_from(index).context("attachment has too many chunks")?,
                ciphertext,
            },
        )?;
        let chunk_delivery = tokio::select! {
            biased;
            () = progress.control.cancelled() => bail!("transfer cancelled"),
            delivery = deliver_file_packet(
                network, conversation_id, recipient, &packet, queued
            ) => delivery?,
        };
        if chunk_delivery == "queued" || (chunk_delivery == "mailbox" && delivery == "direct") {
            delivery = chunk_delivery;
        }
        emit_json(&serde_json::json!({
            "event":"transfer_progress", "transfer_id":progress.id,
            "file_name":file_name,
            "sent_chunks":progress.device_index.saturating_mul(total_chunks) + index + 1,
            "total_chunks":overall_total, "cancelable":true
        }))?;
    }
    Ok((transfer_id, file_name, metadata.len(), delivery))
}

async fn deliver_file_packet(
    network: &PeerNetwork,
    conversation_id: ConversationId,
    recipient: &Contact,
    packet: &ChatPacket,
    queued: &mut Vec<QueuedFilePacket>,
) -> Result<&'static str> {
    let (route, envelope) =
        encrypt_direct_packet(packet, &recipient.shared_secret, recipient.device_id)?;
    if network.send(&recipient.address, &envelope).await.is_ok() {
        return Ok("direct");
    }
    if deposit_to_any_mailbox(&recipient.mailbox_urls, &route, &envelope)
        .await
        .is_ok()
    {
        return Ok("mailbox");
    }
    queued.push(QueuedFilePacket {
        conversation_id,
        event_id: EventId::from_bytes(*blake3::hash(&envelope).as_bytes()),
        recipient: recipient.device_id,
        envelope,
    });
    Ok("queued")
}

async fn send_group_file_packets(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    store: &Store,
    mls: &mut MlsClient,
    group_id: ConversationId,
    path: &Path,
) -> Result<([u8; 32], String, u64, &'static str)> {
    let metadata = std::fs::metadata(path).with_context(|| format!("read {}", path.display()))?;
    if !metadata.is_file() {
        bail!("attachment must be a regular file");
    }
    if metadata.len() > MAX_ATTACHMENT_BYTES {
        bail!("attachment exceeds the 512 MiB client limit");
    }
    let group = profile
        .groups
        .iter()
        .find(|group| group.id == group_id)
        .cloned()
        .context("group not found")?;
    let recipients = group_remote_contacts(profile, &group, key.device_id());
    if recipients.is_empty() {
        bail!("group has no remote members");
    }

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("attachment file name is not valid UTF-8")?
        .to_owned();
    let plaintext = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let mut file_secret = [0_u8; 32];
    OsRng.fill_bytes(&mut file_secret);
    let blob = encrypt_blob(
        &GroupSecret::from_bytes(file_secret),
        &plaintext,
        file_name.clone(),
        "application/octet-stream",
        256 * 1024,
    )?;
    let transfer_id = blob.manifest.ciphertext_hash;
    let total_chunks = blob.chunks.len().saturating_mul(recipients.len());
    let mut sent_chunks = 0_usize;
    emit_json(&serde_json::json!({
        "event":"transfer_progress", "transfer_id":hex::encode(transfer_id),
        "file_name":file_name, "sent_chunks":sent_chunks, "total_chunks":total_chunks,
        "cancelable":false
    }))?;
    let events = store.load_events(group_id)?;
    let frontier = events
        .iter()
        .fold(CausalFrontier::new(), |mut frontier, event| {
            frontier
                .entry(event.author_device)
                .and_modify(|value| *value = (*value).max(event.device_sequence))
                .or_insert(event.device_sequence);
            frontier
        });
    let mut builder =
        ConversationBuilder::new(group_id, profile.identity_id, key.device_id(), frontier);
    let event = builder.build(
        EventBody::MessageCreate {
            content: MessageContent {
                text: format!("📎 {file_name}"),
                reply_to: None,
                attachment_ids: vec![transfer_id],
            },
        },
        next_group_logical_time(store, group_id)?,
        &mut OsRng,
    )?;
    let encrypted_secret = mls.encrypt(group_id.as_bytes(), &file_secret)?;
    store.save_event(&event)?;

    let mut route = "direct";
    for recipient in &recipients {
        let delivery = deliver_payload_durable(
            network,
            key,
            profile,
            store,
            group_id,
            recipient,
            DirectPayload::GroupFileOffer {
                group_id,
                transfer_id,
                encrypted_secret: encrypted_secret.clone(),
                manifest: blob.manifest.clone(),
                event: Box::new(event.clone()),
            },
        )
        .await?;
        match delivery {
            "queued" => route = "queued",
            "mailbox" if route == "direct" => route = "mailbox",
            _ => {}
        }
        for (index, ciphertext) in blob.chunks.iter().enumerate() {
            let delivery = deliver_payload_durable(
                network,
                key,
                profile,
                store,
                group_id,
                recipient,
                DirectPayload::GroupFileChunk {
                    group_id,
                    transfer_id,
                    index: u32::try_from(index).context("attachment has too many chunks")?,
                    ciphertext: ciphertext.clone(),
                },
            )
            .await?;
            match delivery {
                "queued" => route = "queued",
                "mailbox" if route == "direct" => route = "mailbox",
                _ => {}
            }
            sent_chunks = sent_chunks.saturating_add(1);
            emit_json(&serde_json::json!({
                "event":"transfer_progress", "transfer_id":hex::encode(transfer_id),
                "file_name":file_name, "sent_chunks":sent_chunks,
                "total_chunks":total_chunks, "cancelable":false
            }))?;
        }
    }
    Ok((transfer_id, file_name, metadata.len(), route))
}

async fn send_device_history(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    store: &Store,
    recipient: &Contact,
) -> Result<()> {
    let messages = store.load_all_direct_messages()?;
    for chunk in messages.chunks(4) {
        let messages = chunk
            .iter()
            .map(|message| DeviceHistoryMessage {
                message_id: message.message_id,
                peer_identity: message.peer_identity,
                sender_name: message.sender_name.clone(),
                body: message.body.clone(),
                sent_at_unix: message.sent_at_unix,
                outgoing: message.outgoing,
                reply_to: message.reply_to,
                edited: message.edited,
                deleted: message.deleted,
                delivery: message.delivery.clone(),
            })
            .collect();
        deliver_payload(
            network,
            key,
            profile,
            recipient,
            DirectPayload::DeviceHistory { messages },
        )
        .await?;
    }
    Ok(())
}

fn handle_incoming_payload(
    store: &Store,
    profile_path: &Path,
    local_identity: IdentityId,
    packet: &ChatPacket,
    shared_secret: [u8; 32],
    incoming_files: &mut std::collections::BTreeMap<[u8; 32], IncomingFile>,
    delivery: &str,
) -> Result<()> {
    match &packet.payload {
        DirectPayload::DeviceHistory { messages } => {
            if packet.sender_identity != local_identity {
                bail!("history sync is only accepted from another authorized local device");
            }
            for message in messages {
                store.save_direct_message(&DirectMessageRecord {
                    message_id: message.message_id,
                    peer_identity: message.peer_identity,
                    sender_name: message.sender_name.clone(),
                    body: message.body.clone(),
                    sent_at_unix: message.sent_at_unix,
                    outgoing: message.outgoing,
                    reply_to: message.reply_to,
                    edited: message.edited,
                    deleted: message.deleted,
                    delivery: message.delivery.clone(),
                    file_path: None,
                })?;
            }
            emit_json(&serde_json::json!({"event":"history_synced", "messages":messages.len()}))?;
        }
        DirectPayload::Message {
            message_id,
            body,
            reply_to,
        } => {
            let message_id = if *message_id == [0; 32] {
                *blake3::hash(&packet.to_wire()?).as_bytes()
            } else {
                *message_id
            };
            store.save_direct_message(&DirectMessageRecord {
                message_id,
                peer_identity: packet.sender_identity,
                sender_name: packet.sender_name.clone(),
                body: body.clone(),
                sent_at_unix: packet.sent_at_unix,
                outgoing: false,
                reply_to: *reply_to,
                edited: false,
                deleted: false,
                delivery: "delivered".into(),
                file_path: None,
            })?;
            emit_json(&serde_json::json!({
                "event":"message", "message_id":hex::encode(message_id),
                "from":packet.sender_name, "body":body, "reply_to":reply_to.map(hex::encode),
                "sent_at":packet.sent_at_unix, "delivery":delivery
            }))?;
        }
        DirectPayload::MessageEdit { target, body } => {
            if store.update_direct_message(
                *target,
                packet.sender_identity,
                false,
                Some(body),
                false,
            )? {
                emit_json(&serde_json::json!({
                    "event":"message_edited", "contact":packet.sender_name,
                    "message_id":hex::encode(target), "body":body
                }))?;
            }
        }
        DirectPayload::MessageDelete { target } => {
            if store.update_direct_message(*target, packet.sender_identity, false, None, true)? {
                emit_json(&serde_json::json!({
                    "event":"message_deleted", "contact":packet.sender_name,
                    "message_id":hex::encode(target)
                }))?;
            }
        }
        DirectPayload::DeliveryReceipt { target } => {
            if store.set_direct_delivery(*target, "delivered")? {
                emit_json(&serde_json::json!({
                    "event":"message_delivered", "contact":packet.sender_name,
                    "message_id":hex::encode(target)
                }))?;
            }
        }
        DirectPayload::FileOffer {
            transfer_id,
            manifest,
        } => {
            if manifest.ciphertext_hash != *transfer_id || manifest.chunk_hashes.len() > 65_536 {
                bail!("invalid attachment manifest");
            }
            let incoming = incoming_files.entry(*transfer_id).or_default();
            incoming.secret = Some(shared_secret);
            incoming.manifest = Some(manifest.clone());
            emit_json(&serde_json::json!({
                "event":"file_progress", "from":packet.sender_name,
                "file_name":manifest.file_name, "received_chunks":incoming.chunks.len(),
                "total_chunks":manifest.chunk_hashes.len()
            }))?;
            finish_incoming_file(store, profile_path, packet, *transfer_id, incoming_files)?;
        }
        DirectPayload::FileChunk {
            transfer_id,
            index,
            ciphertext,
        } => {
            if ciphertext.len() > 4 * 1024 * 1024 {
                bail!("attachment chunk exceeds protocol limit");
            }
            let incoming = incoming_files.entry(*transfer_id).or_default();
            incoming.secret = Some(shared_secret);
            incoming
                .chunks
                .entry(*index)
                .or_insert_with(|| ciphertext.clone());
            finish_incoming_file(store, profile_path, packet, *transfer_id, incoming_files)?;
        }
        DirectPayload::DeviceHello
        | DirectPayload::GroupWelcome { .. }
        | DirectPayload::GroupMessage { .. }
        | DirectPayload::GroupCommit { .. }
        | DirectPayload::GroupProfileUpdate { .. }
        | DirectPayload::GroupDissolve { .. }
        | DirectPayload::GroupFileOffer { .. }
        | DirectPayload::GroupFileChunk { .. }
        | DirectPayload::GroupSyncRequest { .. }
        | DirectPayload::GroupSyncEvent { .. } => {}
    }
    Ok(())
}

async fn acknowledge_incoming_message(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    profile: &Profile,
    store: &Store,
    packet: &ChatPacket,
) -> Result<()> {
    let DirectPayload::Message { message_id, .. } = &packet.payload else {
        return Ok(());
    };
    if *message_id == [0; 32] {
        return Ok(());
    }
    let Some(recipient) = profile.contacts.iter().find(|contact| {
        contact.identity_id == packet.sender_identity && contact.device_id == packet.sender_device
    }) else {
        return Ok(());
    };
    let receipt = signed_direct_packet(
        network,
        key,
        profile,
        DirectPayload::DeliveryReceipt {
            target: *message_id,
        },
    )?;
    let conversation_id = direct_conversation_id(profile.identity_id, packet.sender_identity);
    deliver_signed_packet_durable(network, store, conversation_id, recipient, &receipt).await?;
    Ok(())
}

fn finish_incoming_file(
    store: &Store,
    profile_path: &Path,
    packet: &ChatPacket,
    transfer_id: [u8; 32],
    incoming_files: &mut std::collections::BTreeMap<[u8; 32], IncomingFile>,
) -> Result<()> {
    let Some(incoming) = incoming_files.get(&transfer_id) else {
        return Ok(());
    };
    let (Some(secret), Some(manifest)) = (incoming.secret, incoming.manifest.clone()) else {
        return Ok(());
    };
    if incoming.chunks.len() != manifest.chunk_hashes.len() {
        return Ok(());
    }
    let mut chunks = Vec::with_capacity(manifest.chunk_hashes.len());
    for index in 0..manifest.chunk_hashes.len() {
        let index = u32::try_from(index).context("attachment index overflow")?;
        let Some(chunk) = incoming.chunks.get(&index) else {
            return Ok(());
        };
        chunks.push(chunk.clone());
    }
    let plaintext = decrypt_blob(
        &GroupSecret::from_bytes(secret),
        &EncryptedBlob {
            manifest: manifest.clone(),
            chunks,
        },
    )?;
    let safe_name = Path::new(&manifest.file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment.bin");
    let directory = profile_path.with_extension("files");
    std::fs::create_dir_all(&directory)?;
    let destination = directory.join(format!("{}-{safe_name}", &hex::encode(transfer_id)[..12]));
    let temporary = destination.with_extension("part");
    std::fs::write(&temporary, plaintext)?;
    std::fs::rename(&temporary, &destination)?;
    incoming_files.remove(&transfer_id);
    store.save_direct_message(&DirectMessageRecord {
        message_id: transfer_id,
        peer_identity: packet.sender_identity,
        sender_name: packet.sender_name.clone(),
        body: format!("📎 {}", manifest.file_name),
        sent_at_unix: packet.sent_at_unix,
        outgoing: false,
        reply_to: None,
        edited: false,
        deleted: false,
        delivery: "delivered".into(),
        file_path: Some(destination.to_string_lossy().into_owned()),
    })?;
    emit_json(&serde_json::json!({
        "event":"file_received", "from":packet.sender_name,
        "file_name":manifest.file_name, "byte_len":manifest.byte_len,
        "path":destination, "sent_at":packet.sent_at_unix
    }))?;
    Ok(())
}

fn emit_contacts(profile: &Profile) -> Result<()> {
    let mut emitted = Vec::<IdentityId>::new();
    let contacts = profile
        .contacts
        .iter()
        .filter(|contact| contact.identity_id != profile.identity_id && !contact.removed)
        .filter_map(|contact| {
            if emitted.contains(&contact.identity_id) {
                return None;
            }
            emitted.push(contact.identity_id);
            let device_count = profile
                .contacts
                .iter()
                .filter(|candidate| candidate.identity_id == contact.identity_id)
                .count();
            Some(serde_json::json!({
                "name":contact.name,
                "avatar":contact.avatar,
                "identity_id":contact.identity_id,
                "device_id":contact.device_id,
                "device_count":device_count,
                "verified":contact.verified,
                "manually_verified":contact.manually_verified,
                "fingerprint":identity_fingerprint(contact.identity_id),
                "blocked":contact.blocked,
                "hide_presence":contact.hide_presence,
                "endpoint_id":contact.address.endpoint_id,
            }))
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"contacts", "contacts":contacts}))
}

fn identity_fingerprint(identity: IdentityId) -> String {
    let compact = identity.to_string().replace('-', "").to_uppercase();
    compact
        .as_bytes()
        .chunks(4)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

fn qr_svg(value: &str) -> Result<String> {
    let code = QrCode::new(value.as_bytes()).context("create invitation QR")?;
    let width = code.width();
    let quiet = 4;
    let size = width + quiet * 2;
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x, y)] == Color::Dark {
                write!(&mut path, "M{} {}h1v1h-1z", x + quiet, y + quiet)
                    .expect("writing to a String cannot fail");
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><path d=\"{path}\" fill=\"#111016\"/></svg>"
    ))
}

fn emit_groups(profile: &Profile) -> Result<()> {
    let groups = profile
        .groups
        .iter()
        .map(|group| {
            serde_json::json!({
                "id":group.id.to_string(), "name":group.name, "owner":group.owner,
                "member_count":group.members.len(), "device_count":group.member_devices.len(),
                "admin_count":group.admins.len(),
                "owned":group.owner == profile.identity_id,
                "admin":group.admins.contains(&profile.identity_id)
            })
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"groups", "groups":groups}))
}

fn emit_conversation_settings(store: &Store) -> Result<()> {
    let settings = store
        .load_conversation_settings()?
        .into_iter()
        .map(|item| serde_json::json!({
            "conversation_key": item.conversation_key,
            "pinned": item.pinned,
            "archived": item.archived,
            "muted": item.muted_until_unix.is_some_and(|until| until > OffsetDateTime::now_utc().unix_timestamp()),
            "unread":item.unread_count,
            "last_summary":item.last_summary,
            "last_activity":item.last_activity_unix,
            "notification_preview":item.notification_preview,
        }))
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"conversation_settings", "settings":settings}))
}

fn emit_devices(profile: &Profile) -> Result<()> {
    let current = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
    let identity = IdentityLog::from_events(profile.identity_id, profile.identity_events.clone())?;
    let devices = identity
        .devices()
        .map(|device| {
            serde_json::json!({
                "id":device.device_id.to_string(), "label":device.label,
                "active":device.revoked_at_unix.is_none(), "current":device.device_id == current
            })
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"devices", "devices":devices}))
}

fn emit_group_history(store: &Store, profile: &Profile, group_id: ConversationId) -> Result<()> {
    let messages = materialize_group_messages(store, profile, group_id)?;
    emit_json(
        &serde_json::json!({"event":"group_history", "group_id":group_id.to_string(), "messages":messages}),
    )
}

fn materialize_group_messages(
    store: &Store,
    profile: &Profile,
    group_id: ConversationId,
) -> Result<Vec<serde_json::Value>> {
    if !profile.groups.iter().any(|group| group.id == group_id) {
        bail!("group not found");
    }
    let mut messages = Vec::new();
    let mut positions = std::collections::BTreeMap::<EventId, usize>::new();
    let mut authors = std::collections::BTreeMap::<EventId, IdentityId>::new();
    for event in store.load_events(group_id)? {
        match event.body {
            EventBody::MessageCreate { content } => {
                let author = if event.author_identity == profile.identity_id {
                    profile.name.clone()
                } else {
                    profile
                        .contacts
                        .iter()
                        .find(|contact| contact.identity_id == event.author_identity)
                        .map_or_else(
                            || event.author_identity.short(),
                            |contact| contact.name.clone(),
                        )
                };
                positions.insert(event.event_id, messages.len());
                authors.insert(event.event_id, event.author_identity);
                messages.push(serde_json::json!({
                    "message_id":event.event_id.to_string(), "author":author, "body":content.text,
                    "sent_at":event.logical_time_ms / 1000,
                    "outgoing":event.author_identity == profile.identity_id,
                    "reply_to":content.reply_to.map(|id| id.to_string()),
                    "edited":false, "deleted":false, "delivery":"delivered"
                }));
            }
            EventBody::MessageEdit { target, content }
                if authors.get(&target) == Some(&event.author_identity) =>
            {
                if let Some(index) = positions.get(&target).copied()
                    && let Some(message) = messages
                        .get_mut(index)
                        .and_then(serde_json::Value::as_object_mut)
                {
                    message.insert("body".into(), content.text.into());
                    message.insert("edited".into(), true.into());
                }
            }
            EventBody::MessageDelete { target }
                if authors.get(&target) == Some(&event.author_identity) =>
            {
                if let Some(index) = positions.get(&target).copied()
                    && let Some(message) = messages
                        .get_mut(index)
                        .and_then(serde_json::Value::as_object_mut)
                {
                    message.insert("body".into(), "".into());
                    message.insert("deleted".into(), true.into());
                    message.insert("edited".into(), false.into());
                }
            }
            _ => {}
        }
    }
    Ok(messages)
}

fn emit_history(store: &Store, contact: &Contact) -> Result<()> {
    let messages = store
        .load_direct_messages(contact.identity_id, 2_000)?
        .iter()
        .map(direct_message_json)
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"history", "contact":contact.name, "messages":messages}))
}

fn emit_call_history(store: &Store, conversation: &str) -> Result<()> {
    let calls = store
        .load_call_events(conversation, 200)?
        .into_iter()
        .map(|call| {
            serde_json::json!({
                "call_id":hex::encode(call.call_id), "direction":call.direction,
                "outcome":call.outcome, "started_at":call.started_at_unix,
                "duration_ms":call.duration_ms,
            })
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({
        "event":"call_history", "conversation":conversation, "calls":calls
    }))
}

fn direct_message_json(message: &DirectMessageRecord) -> serde_json::Value {
    serde_json::json!({
        "message_id":hex::encode(message.message_id),
        "peer_identity":message.peer_identity,
        "author":message.sender_name,
        "body":if message.deleted { "" } else { &message.body },
        "sent_at":message.sent_at_unix,
        "outgoing":message.outgoing,
        "reply_to":message.reply_to.map(hex::encode),
        "edited":message.edited,
        "deleted":message.deleted,
        "delivery":message.delivery,
        "file_path":message.file_path,
    })
}

fn random_message_id() -> [u8; 32] {
    let mut id = [0; 32];
    OsRng.fill_bytes(&mut id);
    id
}

fn parse_message_id(value: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(value).context("message id is not hexadecimal")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("message id must contain 32 bytes"))
}

fn emit_json(value: &serde_json::Value) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

fn decrypt_incoming(profile: &Profile, bytes: &[u8]) -> Result<(ChatPacket, [u8; 32], bool)> {
    let envelope = TransportEnvelope::from_wire(bytes)?;
    if !envelope.verify() {
        bail!("invalid transport envelope");
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let candidates = profile
        .contacts
        .iter()
        .map(|contact| (contact.shared_secret, false))
        .chain(
            profile
                .pending_invites
                .iter()
                .filter(move |pending| pending.expires_unix > now)
                .map(|pending| (pending.shared_secret, true)),
        );
    for (secret, was_pending) in candidates {
        let local_device = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
        let route = routing_capability(&secret, local_device);
        if route != envelope.routing_capability {
            continue;
        }
        let encrypted = EncryptedPayload::from_wire(&envelope.ciphertext)?;
        let plaintext = GroupSecret::from_bytes(secret).decrypt(&encrypted, &route)?;
        let packet = ChatPacket::from_wire(&plaintext)?;
        if packet.version != PROTOCOL_VERSION
            || DeviceId::from_bytes(*blake3::hash(&packet.sender_public_key).as_bytes())
                != packet.sender_device
        {
            bail!("invalid sender device proof");
        }
        let identity =
            IdentityLog::from_events(packet.sender_identity, packet.identity_events.clone())?;
        let authorized = identity
            .active_device(packet.sender_device)
            .is_some_and(|device| device.public_key == packet.sender_public_key);
        if !authorized {
            bail!("sender device is not active in the signed identity log");
        }
        if let Some(contact) = profile
            .contacts
            .iter()
            .filter(|contact| contact.identity_id == packet.sender_identity)
            .max_by_key(|contact| contact.identity_events.len())
            && !contact.identity_events.is_empty()
            && (packet.identity_events.len() < contact.identity_events.len()
                || packet.identity_events[..contact.identity_events.len()]
                    != contact.identity_events)
        {
            bail!("sender identity log is stale or forked");
        }
        let mut signable = packet.clone();
        let signature = std::mem::take(&mut signable.signature);
        DeviceKeyPair::verify_message(&packet.sender_public_key, &signable.to_wire()?, &signature)?;
        return Ok((packet, secret, was_pending));
    }
    bail!("unknown routing capability")
}

fn list_contacts(path: &Path) -> Result<()> {
    let profile = load_profile(path)?;
    for contact in profile
        .contacts
        .into_iter()
        .filter(|contact| contact.identity_id != profile.identity_id)
    {
        println!(
            "{}\t{}\t{}",
            contact.name,
            contact.identity_id.short(),
            contact.address.endpoint_id
        );
    }
    Ok(())
}

fn contact_devices(profile: &Profile, name: &str) -> Result<Vec<Contact>> {
    let identity = profile
        .contacts
        .iter()
        .find(|contact| {
            contact.name.eq_ignore_ascii_case(name) && !contact.removed && !contact.blocked
        })
        .map(|contact| contact.identity_id)
        .context("contact not found")?;
    Ok(profile
        .contacts
        .iter()
        .filter(|contact| contact.identity_id == identity)
        .filter(|contact| !contact.removed && !contact.blocked)
        .cloned()
        .collect())
}

fn group_remote_contacts(
    profile: &Profile,
    group: &GroupProfile,
    local_device: DeviceId,
) -> Vec<Contact> {
    profile
        .contacts
        .iter()
        .filter(|contact| {
            contact.device_id != local_device && group.member_devices.contains(&contact.device_id)
        })
        .cloned()
        .collect()
}

fn resolve_call_recipients(profile: &Profile, target: &str) -> Result<Vec<Contact>> {
    if let Some(contact) = profile.contacts.iter().find(|contact| {
        contact.name.eq_ignore_ascii_case(target) && !contact.removed && !contact.blocked
    }) {
        return Ok(profile
            .contacts
            .iter()
            .filter(|candidate| candidate.identity_id == contact.identity_id)
            .filter(|candidate| !candidate.removed && !candidate.blocked)
            .cloned()
            .collect());
    }
    let group = profile
        .groups
        .iter()
        .find(|group| group.name.eq_ignore_ascii_case(target) || group.id.to_string() == target)
        .context("contact or group not found")?;
    if group.members.len() > 8 {
        bail!("group calls support at most 8 participants");
    }
    let local_device = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
    let recipients = group_remote_contacts(profile, group, local_device);
    if recipients.is_empty() {
        bail!("call has no remote participants");
    }
    Ok(recipients)
}

fn call_participants_json(recipients: &[Contact]) -> Vec<serde_json::Value> {
    recipients
        .iter()
        .map(|recipient| {
            serde_json::json!({
                "name":recipient.name,
                "device_id":recipient.device_id,
                "volume":1.0
            })
        })
        .collect()
}

const fn kind_name(kind: MediaDeviceKind) -> &'static str {
    match kind {
        MediaDeviceKind::AudioInput => "audio_input",
        MediaDeviceKind::AudioOutput => "audio_output",
        MediaDeviceKind::Camera => "camera",
        MediaDeviceKind::Screen => "screen",
        MediaDeviceKind::Window => "window",
    }
}

async fn link_device(path: &Path, label: &str) -> Result<()> {
    let mut profile = load_profile(path)?;
    let network = PeerNetwork::start_with_secret(profile.network_secret).await?;
    let url = make_device_link(&mut profile, label, network.local_address())?;
    save_profile(path, &profile)?;
    println!("{url}");
    network.shutdown().await?;
    Ok(())
}

fn make_device_link(
    profile: &mut Profile,
    label: &str,
    primary_address: PeerAddress,
) -> Result<Url> {
    if label.trim().is_empty() || label.chars().count() > 64 {
        bail!("device label must contain 1-64 characters");
    }
    if profile.identity_events.is_empty() {
        bail!("this legacy profile has no identity log and cannot authorize another device");
    }
    let author = DeviceKeyPair::from_secret_bytes(&profile.device_secret);
    let new_device = DeviceKeyPair::generate(&mut OsRng);
    let mut identity =
        IdentityLog::from_events(profile.identity_id, profile.identity_events.clone())?;
    if identity
        .devices()
        .filter(|device| device.revoked_at_unix.is_none())
        .count()
        >= 5
    {
        bail!("an identity can authorize at most five active devices");
    }
    identity.add_device(
        &author,
        &new_device,
        label.trim(),
        OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    profile.identity_events = identity.events().to_vec();

    let mut network_secret = [0; 32];
    OsRng.fill_bytes(&mut network_secret);
    let mut self_shared_secret = [0; 32];
    OsRng.fill_bytes(&mut self_shared_secret);
    let primary_contact = Contact {
        name: profile.name.clone(),
        avatar: profile.avatar.clone(),
        identity_id: profile.identity_id,
        device_id: author.device_id(),
        public_key: author.public_key(),
        address: primary_address,
        mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
        shared_secret: self_shared_secret,
        verified: true,
        manually_verified: true,
        mls_key_package: profile.mls_key_package.clone(),
        identity_events: profile.identity_events.clone(),
        blocked: false,
        removed: false,
        hide_presence: false,
    };
    let mut linked_contacts = profile
        .contacts
        .iter()
        .filter(|contact| contact.identity_id != profile.identity_id)
        .cloned()
        .collect::<Vec<_>>();
    linked_contacts.push(primary_contact);
    let bundle = DeviceBundle {
        version: PROTOCOL_VERSION,
        expires_unix: (OffsetDateTime::now_utc() + Duration::minutes(10)).unix_timestamp(),
        name: profile.name.clone(),
        avatar: profile.avatar.clone(),
        identity_id: profile.identity_id,
        device_secret: new_device.secret_bytes(),
        network_secret,
        database_key: DatabaseKey::generate().expose_for_profile(),
        mailbox_url: profile.mailbox_url.clone(),
        contacts: linked_contacts,
        // An MLS leaf cannot safely be cloned. Group membership is re-established
        // through normal MLS Add/Welcome sync after the new device comes online.
        groups: vec![],
        identity_events: profile.identity_events.clone(),
        mls_snapshot: None,
    };
    upsert_contact(
        profile,
        Contact {
            name: profile.name.clone(),
            avatar: profile.avatar.clone(),
            identity_id: profile.identity_id,
            device_id: new_device.device_id(),
            public_key: new_device.public_key(),
            address: PeerAddress {
                endpoint_id: String::new(),
                direct_addresses: vec![],
                relay_urls: vec![],
            },
            mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
            shared_secret: self_shared_secret,
            verified: true,
            manually_verified: true,
            mls_key_package: vec![],
            identity_events: profile.identity_events.clone(),
            blocked: false,
            removed: false,
            hide_presence: false,
        },
    );
    let mut capability = [0; 32];
    OsRng.fill_bytes(&mut capability);
    let aad = b"pptalk-device-link-v1";
    let encrypted = GroupSecret::from_bytes(capability).encrypt(&bundle.to_wire()?, aad)?;
    let mut fragment = capability.to_vec();
    fragment.extend_from_slice(&encrypted.to_wire()?);
    let mut url = Url::parse("pptalk://device/v1")?;
    url.set_fragment(Some(&URL_SAFE_NO_PAD.encode(fragment)));
    Ok(url)
}

fn import_device(path: &Path, link: &Url) -> Result<()> {
    if path.exists() {
        bail!("profile already exists: {}", path.display());
    }
    if link.scheme() != "pptalk" || link.host_str() != Some("device") || link.path() != "/v1" {
        bail!("invalid device link");
    }
    let fragment = link.fragment().context("device link has no capability")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(fragment)
        .context("invalid device link encoding")?;
    if bytes.len() <= 32 {
        bail!("invalid device link payload");
    }
    let capability: [u8; 32] = bytes[..32].try_into().expect("checked length");
    let encrypted = EncryptedPayload::from_wire(&bytes[32..])?;
    let plaintext =
        GroupSecret::from_bytes(capability).decrypt(&encrypted, b"pptalk-device-link-v1")?;
    let bundle = DeviceBundle::from_wire(&plaintext)?;
    if bundle.version != PROTOCOL_VERSION
        || bundle.expires_unix <= OffsetDateTime::now_utc().unix_timestamp()
    {
        bail!("device link has expired or uses an unsupported version");
    }
    let identity = IdentityLog::from_events(bundle.identity_id, bundle.identity_events.clone())?;
    let device = DeviceKeyPair::from_secret_bytes(&bundle.device_secret);
    if identity.active_device(device.device_id()).is_none() {
        bail!("device authorization is missing from the identity log");
    }
    let profile = Profile {
        version: PROTOCOL_VERSION,
        name: bundle.name,
        avatar: bundle.avatar,
        identity_id: bundle.identity_id,
        device_secret: bundle.device_secret,
        network_secret: bundle.network_secret,
        database_key: bundle.database_key,
        database_key_in_keyring: false,
        mls_key_package: vec![],
        mailbox_url: bundle.mailbox_url,
        contacts: bundle.contacts,
        pending_invites: vec![],
        identity_events: bundle.identity_events,
        groups: bundle.groups,
    };
    save_profile(path, &profile)?;
    if let Some(snapshot) = bundle.mls_snapshot {
        let store = Store::open(
            path.with_extension("history.sqlite3"),
            &DatabaseKey::from_bytes(profile.database_key),
        )?;
        store.save_mls_state(
            ConversationId::from_bytes([0x6d; 32]),
            &snapshot,
            now_millis(),
        )?;
    }
    println!("{}", profile.identity_id);
    Ok(())
}

fn list_devices(path: &Path) -> Result<()> {
    let profile = load_profile(path)?;
    let identity = IdentityLog::from_events(profile.identity_id, profile.identity_events)?;
    for device in identity.devices() {
        println!(
            "{}\t{}\t{}",
            device.device_id,
            device.label,
            if device.revoked_at_unix.is_some() {
                "revoked"
            } else {
                "active"
            }
        );
    }
    Ok(())
}

fn revoke_device(path: &Path, device_id: &str, reason: &str) -> Result<()> {
    let mut profile = load_profile(path)?;
    revoke_profile_device(&mut profile, device_id, reason)?;
    save_profile(path, &profile)?;
    Ok(())
}

fn revoke_profile_device(profile: &mut Profile, device_id: &str, reason: &str) -> Result<()> {
    let target = device_id.parse::<DeviceId>().context("invalid device id")?;
    let author = DeviceKeyPair::from_secret_bytes(&profile.device_secret);
    if target == author.device_id() {
        bail!("the current device cannot revoke itself");
    }
    let mut identity =
        IdentityLog::from_events(profile.identity_id, profile.identity_events.clone())?;
    identity.revoke_device(
        &author,
        target,
        reason,
        OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    profile.identity_events = identity.events().to_vec();
    Ok(())
}

fn routing_capability(secret: &[u8; 32], recipient_device: DeviceId) -> [u8; 32] {
    let mut input = Vec::with_capacity(77);
    input.extend_from_slice(b"pptalk-route-v1");
    input.extend_from_slice(secret);
    input.extend_from_slice(recipient_device.as_bytes());
    *blake3::hash(&input).as_bytes()
}

fn now_millis() -> i64 {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
}

fn next_outbox_time() -> i64 {
    let candidate = now_millis().saturating_add(2_000);
    let previous = LAST_OUTBOX_TIME_MS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
            Some(candidate.max(previous.saturating_add(1)))
        })
        .unwrap_or_default();
    candidate.max(previous.saturating_add(1))
}

fn mailbox_endpoint(base: &Url, route: &[u8; 32]) -> Result<Url> {
    if !base.username().is_empty() || base.password().is_some() {
        bail!("mailbox URL must not contain credentials");
    }
    let secure = base.scheme() == "https";
    let local_http = base.scheme() == "http"
        && matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if !secure && !local_http {
        bail!("mailbox URL must use HTTPS (HTTP is allowed only on loopback)");
    }
    let mut endpoint = base.clone();
    let prefix = endpoint.path().trim_end_matches('/');
    endpoint.set_path(&format!(
        "{prefix}/v1/mailboxes/{}/messages",
        hex::encode(route)
    ));
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    Ok(endpoint)
}

async fn deposit_to_any_mailbox(bases: &[Url], route: &[u8; 32], envelope: &[u8]) -> Result<()> {
    if bases.is_empty() {
        bail!("recipient has no mailbox fallback");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let mut failures = Vec::new();
    for base in bases {
        let mut endpoint = match mailbox_endpoint(base, route) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                failures.push(format!("{base}: {error}"));
                continue;
            }
        };
        endpoint.query_pairs_mut().append_pair("ttl", "604800");
        match client.post(endpoint).body(envelope.to_vec()).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => failures.push(format!("{base}: HTTP {}", response.status())),
            Err(error) => failures.push(format!("{base}: {error}")),
        }
    }
    bail!("all mailbox deposits failed: {}", failures.join("; "))
}

#[derive(Debug, Deserialize)]
struct MailboxBatch {
    messages: Vec<String>,
}

async fn drain_mailbox(profile: &Profile) -> Result<Vec<Vec<u8>>> {
    let Some(base) = &profile.mailbox_url else {
        return Ok(Vec::new());
    };
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let local_device = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
    let routes = profile
        .contacts
        .iter()
        .map(|contact| routing_capability(&contact.shared_secret, local_device))
        .chain(
            profile
                .pending_invites
                .iter()
                .filter(|pending| pending.expires_unix > now)
                .map(|pending| routing_capability(&pending.shared_secret, local_device)),
        )
        .collect::<BTreeSet<_>>();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let mut envelopes = Vec::new();
    for route in routes {
        let mut endpoint = mailbox_endpoint(base, &route)?;
        endpoint.query_pairs_mut().append_pair("limit", "128");
        let response = client.get(endpoint).send().await?.error_for_status()?;
        let batch = response.json::<MailboxBatch>().await?;
        for encoded in batch.messages {
            envelopes.push(
                STANDARD
                    .decode(encoded)
                    .context("invalid mailbox envelope")?,
            );
        }
    }
    Ok(envelopes)
}

fn upsert_contact(profile: &mut Profile, contact: Contact) {
    if let Some(existing) = profile.contacts.iter_mut().find(|existing| {
        existing.identity_id == contact.identity_id && existing.device_id == contact.device_id
    }) {
        *existing = contact;
    } else {
        profile.contacts.push(contact);
    }
}

fn update_contact_key_package(profile: &mut Profile, packet: &ChatPacket) -> bool {
    let mut changed = false;
    for contact in profile
        .contacts
        .iter_mut()
        .filter(|contact| contact.identity_id == packet.sender_identity)
    {
        if contact.device_id == packet.sender_device
            && !packet.mls_key_package.is_empty()
            && contact.mls_key_package != packet.mls_key_package
        {
            contact.mls_key_package.clone_from(&packet.mls_key_package);
            changed = true;
        }
        if packet.identity_events.len() > contact.identity_events.len() {
            contact.identity_events.clone_from(&packet.identity_events);
            changed = true;
        }
    }
    changed
}

async fn add_authorized_device_to_owned_groups(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    store: &Store,
    profile: &mut Profile,
    mls: &mut MlsClient,
    packet: &ChatPacket,
) -> Result<bool> {
    if packet.mls_key_package.is_empty() {
        return Ok(false);
    }
    let Some(added) = profile
        .contacts
        .iter()
        .find(|contact| {
            contact.identity_id == packet.sender_identity
                && contact.device_id == packet.sender_device
        })
        .cloned()
    else {
        return Ok(false);
    };
    let group_indices = profile
        .groups
        .iter()
        .enumerate()
        .filter(|(_, group)| {
            group.owner == profile.identity_id
                && group.members.contains(&packet.sender_identity)
                && !group.member_devices.contains(&packet.sender_device)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut changed = false;
    for group_index in group_indices {
        let old_group = profile.groups[group_index].clone();
        let (welcome, commit) =
            mls.add_member_with_commit(old_group.id.as_bytes(), &packet.mls_key_package)?;
        profile.groups[group_index]
            .member_devices
            .push(packet.sender_device);
        let updated = profile.groups[group_index].clone();
        for recipient in group_remote_contacts(profile, &old_group, key.device_id()) {
            deliver_payload_durable(
                network,
                key,
                profile,
                store,
                old_group.id,
                &recipient,
                DirectPayload::GroupCommit {
                    group: updated.clone(),
                    commit: commit.clone(),
                },
            )
            .await?;
        }
        deliver_payload_durable(
            network,
            key,
            profile,
            store,
            old_group.id,
            &added,
            DirectPayload::GroupWelcome {
                group: updated,
                welcome,
            },
        )
        .await?;
        changed = true;
    }
    Ok(changed)
}

async fn remove_revoked_devices_from_owned_groups(
    network: &PeerNetwork,
    key: &DeviceKeyPair,
    store: &Store,
    profile: &mut Profile,
    mls: &mut MlsClient,
    identity_id: IdentityId,
    identity_events: &[IdentityEvent],
) -> Result<bool> {
    let log = IdentityLog::from_events(identity_id, identity_events.to_vec())?;
    let identity_devices = identity_events
        .iter()
        .filter_map(|event| match &event.kind {
            IdentityEventKind::Genesis { public_key, .. }
            | IdentityEventKind::AddDevice { public_key, .. } => {
                Some(DeviceId::from_bytes(*blake3::hash(public_key).as_bytes()))
            }
            IdentityEventKind::RevokeDevice { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let group_indices = profile
        .groups
        .iter()
        .enumerate()
        .filter(|(_, group)| {
            group.owner == profile.identity_id && group.members.contains(&identity_id)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut changed = false;
    for group_index in group_indices {
        let old_group = profile.groups[group_index].clone();
        let revoked = old_group
            .member_devices
            .iter()
            .filter(|device| {
                identity_devices.contains(device) && log.active_device(**device).is_none()
            })
            .copied()
            .collect::<Vec<_>>();
        if revoked.is_empty() {
            continue;
        }
        let credentials = revoked
            .iter()
            .map(|device| device.as_bytes().as_slice())
            .collect::<Vec<_>>();
        let commit = mls.remove_members(old_group.id.as_bytes(), &credentials)?;
        profile.groups[group_index]
            .member_devices
            .retain(|device| !revoked.contains(device));
        let updated = profile.groups[group_index].clone();
        for recipient in group_remote_contacts(profile, &updated, key.device_id()) {
            deliver_payload_durable(
                network,
                key,
                profile,
                store,
                old_group.id,
                &recipient,
                DirectPayload::GroupCommit {
                    group: updated.clone(),
                    commit: commit.clone(),
                },
            )
            .await?;
        }
        changed = true;
    }
    Ok(changed)
}

fn refresh_contact_device(
    profile: &mut Profile,
    packet: &ChatPacket,
    shared_secret: [u8; 32],
) -> Result<bool> {
    let Some(template) = profile
        .contacts
        .iter()
        .find(|contact| {
            contact.identity_id == packet.sender_identity && contact.shared_secret == shared_secret
        })
        .cloned()
    else {
        return Ok(false);
    };
    let before = profile.contacts.clone();
    upsert_contact(
        profile,
        Contact {
            name: packet.sender_name.clone(),
            avatar: packet.sender_avatar.clone().or(template.avatar),
            identity_id: packet.sender_identity,
            device_id: packet.sender_device,
            public_key: packet.sender_public_key,
            address: packet.return_address.clone(),
            mailbox_urls: packet.mailbox_urls.clone(),
            shared_secret,
            verified: template.verified,
            manually_verified: template.manually_verified,
            mls_key_package: packet.mls_key_package.clone(),
            identity_events: packet.identity_events.clone(),
            blocked: template.blocked,
            removed: template.removed,
            hide_presence: template.hide_presence,
        },
    );
    let log = IdentityLog::from_events(packet.sender_identity, packet.identity_events.clone())?;
    profile.contacts.retain(|contact| {
        contact.identity_id != packet.sender_identity
            || log.active_device(contact.device_id).is_some()
    });
    Ok(profile.contacts != before)
}

fn load_profile(path: &Path) -> Result<Profile> {
    let bytes = std::fs::read(path).with_context(|| format!("read profile {}", path.display()))?;
    let mut profile: Profile = serde_json::from_slice(&bytes).context("decode profile")?;
    if profile.version != PROTOCOL_VERSION {
        bail!("unsupported profile version {}", profile.version);
    }
    if profile.database_key_in_keyring {
        profile.database_key = read_database_key(&profile)?;
    }
    Ok(profile)
}

const BACKUP_MAGIC: &[u8] = b"PPTALK-BACKUP-1\0";

fn backup_key(passphrase: &str, salt: &[u8; 16]) -> Result<[u8; 32]> {
    if passphrase.chars().count() < 10 {
        bail!("backup passphrase must contain at least 10 characters");
    }
    let mut key = [0_u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|error| anyhow::anyhow!("derive backup encryption key: {error}"))?;
    Ok(key)
}

fn export_identity_backup(profile_path: &Path, output: &Path, passphrase: &str) -> Result<()> {
    let mut profile = load_profile(profile_path)?;
    profile.database_key_in_keyring = false;
    let plaintext = serde_json::to_vec(&profile)?;
    let mut salt = [0_u8; 16];
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let mut key = backup_key(passphrase, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_slice())
        .map_err(|_| anyhow::anyhow!("encrypt identity backup"))?;
    key.fill(0);

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(BACKUP_MAGIC)?;
    file.write_all(&salt)?;
    file.write_all(&nonce)?;
    file.write_all(&ciphertext)?;
    file.sync_all()?;
    std::fs::rename(temporary, output)?;
    Ok(())
}

fn import_identity_backup(profile_path: &Path, input: &Path, passphrase: &str) -> Result<()> {
    if profile_path.exists() {
        bail!("profile already exists: {}", profile_path.display());
    }
    let bytes = std::fs::read(input)
        .with_context(|| format!("read identity backup {}", input.display()))?;
    let header_len = BACKUP_MAGIC.len() + 16 + 24;
    if bytes.len() <= header_len || !bytes.starts_with(BACKUP_MAGIC) {
        bail!("invalid or unsupported pptalk backup");
    }
    let salt: [u8; 16] = bytes[BACKUP_MAGIC.len()..BACKUP_MAGIC.len() + 16]
        .try_into()
        .expect("checked backup salt length");
    let nonce_start = BACKUP_MAGIC.len() + 16;
    let nonce: [u8; 24] = bytes[nonce_start..nonce_start + 24]
        .try_into()
        .expect("checked backup nonce length");
    let mut key = backup_key(passphrase, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), &bytes[header_len..])
        .map_err(|_| anyhow::anyhow!("backup passphrase is incorrect or the file is damaged"))?;
    key.fill(0);
    let mut profile: Profile =
        serde_json::from_slice(&plaintext).context("decode identity backup")?;
    if profile.version != PROTOCOL_VERSION {
        bail!("unsupported profile version {}", profile.version);
    }
    profile.database_key_in_keyring = false;
    save_profile(profile_path, &profile)
}

fn save_profile(path: &Path, profile: &Profile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut persisted = profile.clone();
    if persisted.database_key_in_keyring {
        persisted.database_key.fill(0);
    }
    let bytes = serde_json::to_vec_pretty(&persisted)?;
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn database_key_account(profile: &Profile) -> String {
    let device = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
    format!("{}:{device}:database", profile.identity_id)
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn read_database_key(profile: &Profile) -> Result<[u8; 32]> {
    let entry = keyring::Entry::new("pptalk", &database_key_account(profile))
        .context("open the system secure store")?;
    let secret = entry
        .get_secret()
        .context("read the database key from the system secure store")?;
    secret
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("the database key in the system secure store is invalid"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn read_database_key(_profile: &Profile) -> Result<[u8; 32]> {
    bail!("the system secure store is not supported on this platform")
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn protect_database_key(path: &Path, profile: &mut Profile) -> Result<()> {
    if profile.database_key_in_keyring {
        return Ok(());
    }
    let entry = keyring::Entry::new("pptalk", &database_key_account(profile))
        .context("open the system secure store")?;
    entry
        .set_secret(&profile.database_key)
        .context("save the database key in the system secure store")?;
    let restored = entry
        .get_secret()
        .context("verify the database key in the system secure store")?;
    if restored.as_slice() != profile.database_key {
        bail!("the system secure store did not return the saved database key");
    }
    profile.database_key_in_keyring = true;
    save_profile(path, profile)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn protect_database_key(_path: &Path, _profile: &mut Profile) -> Result<()> {
    bail!("the system secure store is not supported on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_profile(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pptalk-cli-{label}-{}.json", uuid::Uuid::new_v4()))
    }

    #[test]
    fn release_versions_compare_numerically() {
        assert!(release_version("v0.10.0") > release_version("0.9.9"));
        assert_eq!(release_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(release_version("not-a-version"), None);
    }

    #[test]
    fn profile_roundtrip_and_packet_signature() {
        let path = temporary_profile("roundtrip");
        initialize(&path, "Alice".into(), None).expect("init");
        let profile = load_profile(&path).expect("load");
        assert_eq!(profile.name, "Alice");
        let device = DeviceId::from_bytes([3; 32]);
        assert_ne!(
            routing_capability(&[1; 32], device),
            routing_capability(&[2; 32], device)
        );
        assert_ne!(
            routing_capability(&[1; 32], device),
            routing_capability(&[1; 32], DeviceId::from_bytes([4; 32]))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encrypted_identity_backup_restores_without_exposing_plaintext() {
        let source = temporary_profile("backup-source");
        let restored = temporary_profile("backup-restored");
        let backup = temporary_profile("identity-backup").with_extension("pptalk-backup");
        initialize(&source, "Alice Backup".into(), None).expect("init");
        export_identity_backup(&source, &backup, "correct horse battery").expect("export backup");
        let bytes = std::fs::read(&backup).expect("read backup");
        assert!(bytes.starts_with(BACKUP_MAGIC));
        assert!(!String::from_utf8_lossy(&bytes).contains("Alice Backup"));
        assert!(import_identity_backup(&restored, &backup, "wrong passphrase").is_err());
        import_identity_backup(&restored, &backup, "correct horse battery")
            .expect("restore backup");
        let restored_profile = load_profile(&restored).expect("load restored profile");
        assert_eq!(restored_profile.name, "Alice Backup");

        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(restored);
        let _ = std::fs::remove_file(backup);
    }

    #[test]
    fn secure_storage_profile_writes_only_a_redacted_database_key() {
        let path = temporary_profile("secure-storage-redaction");
        initialize(&path, "Alice".into(), None).expect("init");
        let mut profile = load_profile(&path).expect("load");
        assert_ne!(profile.database_key, [0; 32]);
        profile.database_key_in_keyring = true;
        save_profile(&path, &profile).expect("save redacted profile");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        let key = persisted["database_key"].as_array().expect("database key");
        assert!(key.iter().all(|value| value.as_u64() == Some(0)));
        assert_eq!(persisted["database_key_in_keyring"], true);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fingerprints_are_stable_and_easy_to_compare_in_blocks() {
        let identity = IdentityId::from_bytes([0xab; 32]);
        let fingerprint = identity_fingerprint(identity);
        assert_eq!(fingerprint.replace(' ', "").len(), 64);
        assert!(fingerprint.split(' ').all(|block| block.len() == 4));
        assert_eq!(fingerprint, identity_fingerprint(identity));
    }

    #[test]
    fn linked_device_has_independent_keys_and_a_signed_authorization() {
        let primary_path = temporary_profile("primary");
        let linked_path = temporary_profile("linked");
        initialize(&primary_path, "Alice".into(), None).expect("init primary");
        let mut primary = load_profile(&primary_path).expect("load primary");
        let link = make_device_link(
            &mut primary,
            "Laptop",
            PeerAddress {
                endpoint_id: "test-primary".into(),
                direct_addresses: vec![],
                relay_urls: vec![],
            },
        )
        .expect("create device link");
        save_profile(&primary_path, &primary).expect("persist authorization");
        import_device(&linked_path, &link).expect("import device");
        let linked = load_profile(&linked_path).expect("load linked");

        assert_eq!(primary.identity_id, linked.identity_id);
        assert_ne!(primary.device_secret, linked.device_secret);
        assert_ne!(primary.network_secret, linked.network_secret);
        assert_ne!(primary.database_key, linked.database_key);
        assert!(linked.groups.is_empty());
        let log = IdentityLog::from_events(linked.identity_id, linked.identity_events.clone())
            .expect("valid linked identity log");
        let linked_device = DeviceKeyPair::from_secret_bytes(&linked.device_secret).device_id();
        assert!(log.active_device(linked_device).is_some());

        let _ = std::fs::remove_file(primary_path);
        let _ = std::fs::remove_file(linked_path);
    }
}
