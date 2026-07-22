use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use clap::{Parser, Subcommand};
use pptalk_core::{
    ConversationBuilder, DeviceKeyPair, EncryptedBlob, EncryptedPayload, GroupSecret,
    IdentityEvent, IdentityEventKind, IdentityLog, decrypt_blob, encrypt_blob, sign_invite,
    verify_invite,
};
use pptalk_media::{GstMediaEngine, MediaEngine};
use pptalk_mls::{MlsClient, MlsError};
use pptalk_network::{PeerAddress, PeerNetwork};
use pptalk_protocol::{
    BlobManifest, CallId, CallSignal, CausalFrontier, ContactInvite, ConversationEvent,
    ConversationId, DeviceId, EventBody, EventId, IdentityId, MediaDatagram, MediaKind,
    MediaSignal, MessageContent, PROTOCOL_VERSION, QualityMode, QualityProfile, ReachabilityRecord,
    TransportEnvelope, WireDecode, WireEncode,
};
use pptalk_storage::{DatabaseKey, DirectMessageRecord, Store};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use url::Url;

static LAST_OUTBOX_TIME_MS: AtomicI64 = AtomicI64::new(0);

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
    identity_id: IdentityId,
    device_secret: [u8; 32],
    network_secret: [u8; 32],
    database_key: [u8; 32],
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
struct Contact {
    name: String,
    identity_id: IdentityId,
    device_id: DeviceId,
    public_key: [u8; 32],
    address: PeerAddress,
    #[serde(default)]
    mailbox_urls: Vec<Url>,
    shared_secret: [u8; 32],
    verified: bool,
    #[serde(default)]
    mls_key_package: Vec<u8>,
    #[serde(default)]
    identity_events: Vec<IdentityEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupProfile {
    id: ConversationId,
    name: String,
    owner: IdentityId,
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
    Message {
        body: String,
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

#[derive(Debug, Default)]
struct IncomingFile {
    secret: Option<[u8; 32]>,
    manifest: Option<BlobManifest>,
    group_id: Option<ConversationId>,
    group_event: Option<ConversationEvent>,
    chunks: std::collections::BTreeMap<u32, Vec<u8>>,
}

#[derive(Debug, Clone)]
struct ActiveCall {
    id: CallId,
    label: String,
    recipients: Vec<Contact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceBundle {
    version: u16,
    expires_unix: i64,
    name: String,
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
        Command::Daemon { profile } => Box::pin(daemon(&profile)).await,
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum DaemonCommand {
    Contacts,
    Groups,
    Devices,
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
    Invite {
        expires_seconds: Option<i64>,
    },
    Accept {
        url: Url,
    },
    Send {
        contact: String,
        message: String,
    },
    SendFile {
        contact: String,
        path: PathBuf,
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
    StartCall {
        contact: String,
        ring: bool,
    },
    JoinCall {
        contact: String,
        call_id: String,
    },
    LeaveCall {
        contact: String,
        call_id: String,
    },
    SetMedia {
        contact: String,
        call_id: String,
        kind: MediaKind,
        enabled: bool,
        profile: Option<QualityProfile>,
    },
    Shutdown,
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
        identity_id: identity.identity_id(),
        device_secret: key.secret_bytes(),
        network_secret,
        database_key: DatabaseKey::generate().expose_for_profile(),
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
            identity_id: invite.inviter_identity,
            device_id: invite.inviter_device,
            public_key: invite.inviter_device_public_key,
            address,
            mailbox_urls: invite.reachability.mailbox_candidates,
            shared_secret: invite.one_time_secret,
            verified: true,
            mls_key_package: vec![],
            identity_events: vec![],
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
            body: message.into(),
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
                                identity_id: packet.sender_identity,
                                device_id: packet.sender_device,
                                public_key: packet.sender_public_key,
                                address: packet.return_address.clone(),
                                mailbox_urls: packet.mailbox_urls.clone(),
                                shared_secret,
                                verified: true,
                                mls_key_package: packet.mls_key_package.clone(),
                                identity_events: packet.identity_events.clone(),
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
            _ = mailbox_tick.tick() => {
                match drain_mailbox(&profile).await {
                    Ok(messages) => for bytes in messages {
                        match decrypt_incoming(&profile, &bytes) {
                            Ok((packet, shared_secret, was_pending)) => {
                                if was_pending {
                                    upsert_contact(&mut profile, Contact {
                                        name: packet.sender_name.clone(),
                                        identity_id: packet.sender_identity,
                                        device_id: packet.sender_device,
                                        public_key: packet.sender_public_key,
                                        address: packet.return_address.clone(),
                                        mailbox_urls: packet.mailbox_urls.clone(),
                                        shared_secret,
                                        verified: true,
                                        mls_key_package: packet.mls_key_package.clone(),
                                        identity_events: packet.identity_events.clone(),
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
    let mut media_tasks: Vec<(MediaKind, tokio::task::JoinHandle<()>)> = Vec::new();
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
        "address":network.local_address()
    }))?;
    emit_contacts(&profile)?;
    emit_groups(&profile)?;
    emit_devices(&profile)?;
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
                    DaemonCommand::Contacts => emit_contacts(&profile)?,
                    DaemonCommand::Groups => emit_groups(&profile)?,
                    DaemonCommand::Devices => emit_devices(&profile)?,
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
                        emit_json(&serde_json::json!({"event":"invite", "url":invite.to_url()?, "expires_unix":expires}))?;
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
                        upsert_contact(&mut profile, Contact {
                            name: invite.display_name,
                            identity_id: invite.inviter_identity,
                            device_id: invite.inviter_device,
                            public_key: invite.inviter_device_public_key,
                            address,
                            mailbox_urls: invite.reachability.mailbox_candidates,
                            shared_secret: invite.one_time_secret,
                            verified: true,
                            mls_key_package: vec![],
                            identity_events: vec![],
                        });
                        save_profile(path, &profile)?;
                        emit_contacts(&profile)?;
                    }
                    DaemonCommand::Send { contact, message } => {
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
                        let mut packet = ChatPacket {
                            version: PROTOCOL_VERSION,
                            sender_name: profile.name.clone(),
                            sender_identity: profile.identity_id,
                            sender_device: key.device_id(),
                            sender_public_key: key.public_key(),
                            identity_events: profile.identity_events.clone(),
                            return_address: network.local_address(),
                            mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
                            mls_key_package: profile.mls_key_package.clone(),
                            sent_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
                            payload: DirectPayload::Message { body: message.clone() },
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
                                let packet_bytes = packet.to_wire()?;
                                store.save_direct_message(&DirectMessageRecord {
                                    message_id: *blake3::hash(&packet_bytes).as_bytes(),
                                    peer_identity: recipients[0].identity_id,
                                    sender_name: profile.name.clone(),
                                    body: message.clone(),
                                    sent_at_unix: packet.sent_at_unix,
                                    outgoing: true,
                                })?;
                                emit_json(&serde_json::json!({"event":"message_sent", "to":recipients[0].name, "devices":recipients.len(), "body":message, "sent_at":packet.sent_at_unix, "delivery":delivery}))?;
                            }
                            Err(error) => emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?,
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
                        let result = async {
                            let mut first = None;
                            let mut route = "direct";
                            for recipient in &recipients {
                                let (transfer_id, file_name, byte_len, delivery) =
                                    send_file_packets(&network, &key, &profile, &store, recipient, &file_path).await?;
                                match delivery {
                                    "queued" => route = "queued",
                                    "mailbox" if route == "direct" => route = "mailbox",
                                    _ => {}
                                }
                                first.get_or_insert((transfer_id, file_name, byte_len));
                            }
                            let (transfer_id, file_name, byte_len) = first.context("contact has no active devices")?;
                            Ok::<_, anyhow::Error>((transfer_id, file_name, byte_len, route))
                        }.await;
                        match result {
                            Ok((transfer_id, file_name, byte_len, delivery)) => {
                                let sent_at = OffsetDateTime::now_utc().unix_timestamp();
                                store.save_direct_message(&DirectMessageRecord {
                                    message_id: transfer_id,
                                    peer_identity: recipients[0].identity_id,
                                    sender_name: profile.name.clone(),
                                    body: format!("📎 {file_name}"),
                                    sent_at_unix: sent_at,
                                    outgoing: true,
                                })?;
                                emit_json(&serde_json::json!({"event":"file_sent", "to":recipients[0].name, "devices":recipients.len(), "file_name":file_name, "byte_len":byte_len, "sent_at":sent_at, "delivery":delivery}))?;
                            }
                            Err(error) => emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?,
                        }
                    }
                    DaemonCommand::CreateGroup { name, members } => {
                        if name.trim().is_empty() || name.chars().count() > 128 || members.is_empty() {
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
                        let group = GroupProfile { id: group_id, name: name.trim().into(), owner: profile.identity_id, members: identities, member_devices, history_since_ms };
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
                    }
                    DaemonCommand::GroupSend { group_id, message } => {
                        let group_id = daemon_or_continue!(group_id
                            .parse::<ConversationId>().context("invalid group id"));
                        if message.is_empty() || message.len() > 64 * 1024 {
                            emit_json(&serde_json::json!({"event":"error", "message":"message must contain 1 byte to 64 KiB"}))?;
                            continue;
                        }
                        let group = daemon_or_continue!(profile.groups.iter()
                            .find(|group| group.id == group_id).cloned().context("group not found"));
                        let events = store.load_events(group_id)?;
                        let frontier = events.iter().fold(CausalFrontier::new(), |mut frontier, event| {
                            frontier.entry(event.author_device).and_modify(|value| *value = (*value).max(event.device_sequence)).or_insert(event.device_sequence);
                            frontier
                        });
                        let mut builder = ConversationBuilder::new(group_id, profile.identity_id, key.device_id(), frontier);
                        let event = builder.build(EventBody::MessageCreate { content: MessageContent {
                            text: message.clone(), reply_to: None, attachment_ids: vec![],
                        }}, next_group_logical_time(&store, group_id)?, &mut OsRng)?;
                        let ciphertext = mls.encrypt(group_id.as_bytes(), &event.to_wire()?)?;
                        store.save_event(&event)?;
                        let mut route = "direct";
                        for recipient in group_remote_contacts(&profile, &group, key.device_id()) {
                            let delivery = deliver_payload_durable(
                                &network, &key, &profile, &store, group_id, &recipient,
                                DirectPayload::GroupMessage { group_id, ciphertext: ciphertext.clone() },
                            ).await?;
                            match delivery {
                                "queued" => route = "queued",
                                "mailbox" if route == "direct" => route = "mailbox",
                                _ => {}
                            }
                        }
                        persist_mls(&store, mls_snapshot_id, &mls)?;
                        emit_json(&serde_json::json!({"event":"group_message", "group_id":group_id.to_string(), "author":profile.name, "body":message, "outgoing":true, "delivery":route}))?;
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
                        if profile.groups[group_index].owner != profile.identity_id {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the group creator can add members"}))?;
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
                        if profile.groups[group_index].owner != profile.identity_id {
                            emit_json(&serde_json::json!({"event":"error", "message":"only the group creator can remove members"}))?;
                            continue;
                        }
                        let removed = daemon_or_continue!(profile.contacts.iter()
                            .find(|item| item.name.eq_ignore_ascii_case(&contact)).cloned().context("contact not found"));
                        if !profile.groups[group_index].members.contains(&removed.identity_id) {
                            emit_json(&serde_json::json!({"event":"error", "message":"contact is not a member"}))?;
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
                    DaemonCommand::StartCall { contact, ring } => {
                        let recipients = daemon_or_continue!(resolve_call_recipients(&profile, &contact));
                        let call_id = CallId::random(&mut OsRng);
                        let signal = CallSignal::Invite {
                            call_id,
                            selected: recipients.iter().map(|recipient| recipient.identity_id).collect(),
                            ring,
                        };
                        for recipient in &recipients {
                            daemon_or_continue!(network.send_call_signal(&recipient.address, &signal).await);
                        }
                        active_call = Some(ActiveCall { id: call_id, label: contact.clone(), recipients });
                        emit_json(&serde_json::json!({"event":"call_started", "call_id":call_id.to_string(), "contact":contact, "ring":ring}))?;
                    }
                    DaemonCommand::JoinCall { contact, call_id } => {
                        let call_id = daemon_or_continue!(call_id.parse::<CallId>().context("invalid call id"));
                        let recipients = daemon_or_continue!(resolve_call_recipients(&profile, &contact));
                        for recipient in &recipients {
                            daemon_or_continue!(network.send_call_signal(&recipient.address, &CallSignal::Join { call_id }).await);
                        }
                        active_call = Some(ActiveCall { id: call_id, label: contact.clone(), recipients });
                        emit_json(&serde_json::json!({"event":"call_joined", "call_id":call_id.to_string(), "contact":contact}))?;
                    }
                    DaemonCommand::LeaveCall { contact, call_id } => {
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
                        active_call = None;
                        emit_json(&serde_json::json!({"event":"call_left", "call_id":call_id.to_string(), "contact":contact}))?;
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
                    DaemonCommand::Shutdown => break,
                }
            }
            incoming = network.receive() => {
                let incoming = incoming?;
                match decrypt_incoming(&profile, &incoming.bytes) {
                    Ok((packet, shared_secret, was_pending)) => {
                        if was_pending {
                            upsert_contact(&mut profile, Contact {
                                name: packet.sender_name.clone(), identity_id: packet.sender_identity,
                                device_id: packet.sender_device, public_key: packet.sender_public_key,
                                address: packet.return_address.clone(), mailbox_urls: packet.mailbox_urls.clone(),
                                shared_secret, verified: true,
                                mls_key_package: packet.mls_key_package.clone(),
                                identity_events: packet.identity_events.clone(),
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
                        if handle_group_payload(
                            &network, &key, &store, path, &mut profile, &mut mls, &packet,
                            &mut incoming_files, "direct",
                        ).await? {
                            persist_mls(&store, mls_snapshot_id, &mls)?;
                        } else {
                            handle_incoming_payload(&store, path, &packet, shared_secret, &mut incoming_files, "direct")?;
                        }
                    }
                    Err(error) => emit_json(&serde_json::json!({"event":"rejected", "message":error.to_string()}))?,
                }
            }
            _ = mailbox_tick.tick() => {
                match drain_mailbox(&profile).await {
                    Ok(messages) => for bytes in messages {
                        match decrypt_incoming(&profile, &bytes) {
                            Ok((packet, shared_secret, was_pending)) => {
                                if was_pending {
                                    upsert_contact(&mut profile, Contact {
                                        name: packet.sender_name.clone(), identity_id: packet.sender_identity,
                                        device_id: packet.sender_device, public_key: packet.sender_public_key,
                                        address: packet.return_address.clone(), mailbox_urls: packet.mailbox_urls.clone(),
                                        shared_secret, verified: true,
                                        mls_key_package: packet.mls_key_package.clone(),
                                        identity_events: packet.identity_events.clone(),
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
                                if handle_group_payload(
                                    &network, &key, &store, path, &mut profile, &mut mls, &packet,
                                    &mut incoming_files, "mailbox",
                                ).await? {
                                    persist_mls(&store, mls_snapshot_id, &mls)?;
                                } else {
                                    handle_incoming_payload(&store, path, &packet, shared_secret, &mut incoming_files, "mailbox")?;
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
                        let group_name = remote_contact.and_then(|sender| profile.groups.iter().find(|group| {
                            group.members.contains(&profile.identity_id)
                                && group.members.contains(&sender.identity_id)
                                && selected.iter().all(|identity| group.members.contains(identity))
                                && group.members.len() > 2
                        })).map(|group| group.name.clone());
                        let target = group_name.or_else(|| remote_contact.map(|contact| contact.name.clone()));
                        emit_json(&serde_json::json!({
                            "event":"call_invite", "call_id":call_id.to_string(), "selected":selected,
                            "ring":ring, "remote_endpoint":remote_endpoint, "contact":target
                        }))?;
                    }
                    CallSignal::Join { call_id } => emit_json(&serde_json::json!({"event":"call_join", "call_id":call_id.to_string(), "remote_endpoint":remote_endpoint}))?,
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
                            emit_json(&serde_json::json!({"event":"call_participant_leave", "call_id":call_id.to_string(), "remote_endpoint":remote_endpoint}))?;
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
                    contact.address.endpoint_id == incoming.remote_endpoint_id
                        && contact.device_id == packet.sender
                });
                if packet.version != PROTOCOL_VERSION || !known_sender {
                    emit_json(&serde_json::json!({"event":"rejected", "message":"unauthorized media datagram"}))?;
                    continue;
                }
                if active_call.as_ref().is_none_or(|call| call.id != packet.call_id) {
                    continue;
                }
                if let Err(error) = media.receive_rtp_packet(packet.kind, packet.payload).await {
                    emit_json(&serde_json::json!({"event":"error", "message":error.to_string()}))?;
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
        DirectPayload::Message { body } => Some(body),
        DirectPayload::DeviceHello
        | DirectPayload::FileOffer { .. }
        | DirectPayload::FileChunk { .. }
        | DirectPayload::GroupWelcome { .. }
        | DirectPayload::GroupMessage { .. }
        | DirectPayload::GroupCommit { .. }
        | DirectPayload::GroupFileOffer { .. }
        | DirectPayload::GroupFileChunk { .. }
        | DirectPayload::GroupSyncRequest { .. }
        | DirectPayload::GroupSyncEvent { .. } => None,
    }
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
            let inserted = store.save_event(&event)?;
            if inserted && let EventBody::MessageCreate { content } = event.body {
                emit_json(&serde_json::json!({
                    "event":"group_message", "group_id":group_id.to_string(),
                    "author":packet.sender_name, "body":content.text, "outgoing":false
                }))?;
            }
            Ok(true)
        }
        DirectPayload::GroupCommit { group, commit } => {
            let Some(index) = profile.groups.iter().position(|known| known.id == group.id) else {
                bail!("commit for unknown group");
            };
            if profile.groups[index].owner != packet.sender_identity
                || group.owner != packet.sender_identity
            {
                bail!("only the group creator can commit membership changes");
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
        DirectPayload::GroupFileOffer {
            group_id,
            transfer_id,
            encrypted_secret,
            manifest,
            event,
        } => {
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
            if store.save_event(&event)?
                && let EventBody::MessageCreate { content } = event.body
            {
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
                    "author":author, "body":content.text, "outgoing":outgoing, "synced":true
                }))?;
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
    store: &Store,
    recipient: &Contact,
    path: &Path,
) -> Result<([u8; 32], String, u64, &'static str)> {
    let metadata = std::fs::metadata(path).with_context(|| format!("read {}", path.display()))?;
    if !metadata.is_file() {
        bail!("attachment must be a regular file");
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
    let mut delivery =
        deliver_signed_packet_durable(network, store, conversation_id, recipient, &offer).await?;
    for (index, ciphertext) in blob.chunks.into_iter().enumerate() {
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
        let chunk_delivery =
            deliver_signed_packet_durable(network, store, conversation_id, recipient, &packet)
                .await?;
        if chunk_delivery == "queued" || (chunk_delivery == "mailbox" && delivery == "direct") {
            delivery = chunk_delivery;
        }
    }
    Ok((transfer_id, file_name, metadata.len(), delivery))
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
    const MAX_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;
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
        }
    }
    Ok((transfer_id, file_name, metadata.len(), route))
}

fn handle_incoming_payload(
    store: &Store,
    profile_path: &Path,
    packet: &ChatPacket,
    shared_secret: [u8; 32],
    incoming_files: &mut std::collections::BTreeMap<[u8; 32], IncomingFile>,
    delivery: &str,
) -> Result<()> {
    match &packet.payload {
        DirectPayload::Message { body } => {
            store.save_direct_message(&DirectMessageRecord {
                message_id: *blake3::hash(&packet.to_wire()?).as_bytes(),
                peer_identity: packet.sender_identity,
                sender_name: packet.sender_name.clone(),
                body: body.clone(),
                sent_at_unix: packet.sent_at_unix,
                outgoing: false,
            })?;
            emit_json(&serde_json::json!({
                "event":"message", "from":packet.sender_name, "body":body,
                "sent_at":packet.sent_at_unix, "delivery":delivery
            }))?;
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
        | DirectPayload::GroupFileOffer { .. }
        | DirectPayload::GroupFileChunk { .. }
        | DirectPayload::GroupSyncRequest { .. }
        | DirectPayload::GroupSyncEvent { .. } => {}
    }
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
        .filter(|contact| contact.identity_id != profile.identity_id)
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
                "identity_id":contact.identity_id,
                "device_id":contact.device_id,
                "device_count":device_count,
                "verified":contact.verified,
                "endpoint_id":contact.address.endpoint_id,
            }))
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"contacts", "contacts":contacts}))
}

fn emit_groups(profile: &Profile) -> Result<()> {
    let groups = profile
        .groups
        .iter()
        .map(|group| {
            serde_json::json!({
                "id":group.id.to_string(), "name":group.name, "owner":group.owner,
                "member_count":group.members.len(), "device_count":group.member_devices.len(),
                "owned":group.owner == profile.identity_id
            })
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"groups", "groups":groups}))
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
    if !profile.groups.iter().any(|group| group.id == group_id) {
        bail!("group not found");
    }
    let messages = store
        .load_events(group_id)?
        .into_iter()
        .filter_map(|event| match event.body {
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
                Some(serde_json::json!({
                    "message_id":event.event_id.to_string(), "author":author, "body":content.text,
                    "sent_at":event.logical_time_ms / 1000,
                    "outgoing":event.author_identity == profile.identity_id
                }))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    emit_json(
        &serde_json::json!({"event":"group_history", "group_id":group_id.to_string(), "messages":messages}),
    )
}

fn emit_history(store: &Store, contact: &Contact) -> Result<()> {
    let messages = store
        .load_direct_messages(contact.identity_id, 2_000)?
        .into_iter()
        .map(|message| {
            serde_json::json!({
                "message_id":hex::encode(message.message_id),
                "author":message.sender_name,
                "body":message.body,
                "sent_at":message.sent_at_unix,
                "outgoing":message.outgoing,
            })
        })
        .collect::<Vec<_>>();
    emit_json(&serde_json::json!({"event":"history", "contact":contact.name, "messages":messages}))
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
        .find(|contact| contact.name.eq_ignore_ascii_case(name))
        .map(|contact| contact.identity_id)
        .context("contact not found")?;
    Ok(profile
        .contacts
        .iter()
        .filter(|contact| contact.identity_id == identity)
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
    if let Some(contact) = profile
        .contacts
        .iter()
        .find(|contact| contact.name.eq_ignore_ascii_case(target))
    {
        return Ok(profile
            .contacts
            .iter()
            .filter(|candidate| candidate.identity_id == contact.identity_id)
            .cloned()
            .collect());
    }
    let group = profile
        .groups
        .iter()
        .find(|group| group.name.eq_ignore_ascii_case(target) || group.id.to_string() == target)
        .context("contact or group not found")?;
    let local_device = DeviceKeyPair::from_secret_bytes(&profile.device_secret).device_id();
    let recipients = group_remote_contacts(profile, group, local_device);
    if recipients.is_empty() {
        bail!("call has no remote participants");
    }
    Ok(recipients)
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
        identity_id: profile.identity_id,
        device_id: author.device_id(),
        public_key: author.public_key(),
        address: primary_address,
        mailbox_urls: profile.mailbox_url.clone().into_iter().collect(),
        shared_secret: self_shared_secret,
        verified: true,
        mls_key_package: profile.mls_key_package.clone(),
        identity_events: profile.identity_events.clone(),
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
            mls_key_package: vec![],
            identity_events: profile.identity_events.clone(),
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
        identity_id: bundle.identity_id,
        device_secret: bundle.device_secret,
        network_secret: bundle.network_secret,
        database_key: bundle.database_key,
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
            identity_id: packet.sender_identity,
            device_id: packet.sender_device,
            public_key: packet.sender_public_key,
            address: packet.return_address.clone(),
            mailbox_urls: packet.mailbox_urls.clone(),
            shared_secret,
            verified: template.verified,
            mls_key_package: packet.mls_key_package.clone(),
            identity_events: packet.identity_events.clone(),
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
    let profile: Profile = serde_json::from_slice(&bytes).context("decode profile")?;
    if profile.version != PROTOCOL_VERSION {
        bail!("unsupported profile version {}", profile.version);
    }
    Ok(profile)
}

fn save_profile(path: &Path, profile: &Profile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(profile)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_profile(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pptalk-cli-{label}-{}.json", uuid::Uuid::new_v4()))
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
