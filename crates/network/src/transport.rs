use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use iroh::{
    Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, TransportAddr,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use pptalk_protocol::MAX_ENVELOPE_BYTES;
use tokio::sync::{Mutex, Notify, mpsc};

const INTERACTIVE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Serializable addressing data carried in contact and device records.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PeerAddress {
    pub endpoint_id: String,
    pub direct_addresses: Vec<SocketAddr>,
    pub relay_urls: Vec<String>,
}

impl PeerAddress {
    fn to_iroh(&self) -> Result<EndpointAddr, NetworkError> {
        let endpoint_id = PublicKey::from_str(&self.endpoint_id)
            .map_err(|error| NetworkError::InvalidAddress(error.to_string()))?;
        let mut addresses = self
            .direct_addresses
            .iter()
            .copied()
            .map(TransportAddr::Ip)
            .collect::<Vec<_>>();
        for relay in &self.relay_urls {
            let url = RelayUrl::from_str(relay)
                .map_err(|error| NetworkError::InvalidAddress(error.to_string()))?;
            addresses.push(TransportAddr::Relay(url));
        }
        Ok(EndpointAddr::from_parts(endpoint_id, addresses))
    }
}

impl From<EndpointAddr> for PeerAddress {
    fn from(value: EndpointAddr) -> Self {
        Self {
            endpoint_id: value.id.to_string(),
            direct_addresses: value.ip_addrs().copied().collect(),
            relay_urls: value.relay_urls().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingEnvelope {
    pub remote_endpoint_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMediaDatagram {
    pub remote_endpoint_id: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MediaSession {
    connection: Connection,
    remote_endpoint_id: String,
}

impl MediaSession {
    pub async fn send(&self, bytes: Vec<u8>) -> Result<(), NetworkError> {
        self.connection
            .send_datagram_wait(bytes.into())
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))
    }

    pub fn max_datagram_size(&self) -> Option<usize> {
        self.connection.max_datagram_size()
    }

    pub fn remote_endpoint_id(&self) -> &str {
        &self.remote_endpoint_id
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("invalid peer address: {0}")]
    InvalidAddress(String),
    #[error("transport operation failed: {0}")]
    Transport(String),
    #[error("protocol operation failed: {0}")]
    Protocol(String),
    #[error("incoming transport queue has closed")]
    IncomingClosed,
}

/// Authenticated QUIC endpoint. Iroh attempts direct UDP paths first and can
/// fall back to an encrypted relay path without exposing message plaintext.
#[derive(Debug, Clone)]
pub struct PeerNetwork {
    router: Router,
    incoming: Arc<Mutex<mpsc::Receiver<IncomingEnvelope>>>,
    calls: Arc<Mutex<mpsc::Receiver<IncomingEnvelope>>>,
    media: Arc<Mutex<mpsc::Receiver<IncomingMediaDatagram>>>,
    media_sessions: Arc<MediaSessionRegistry>,
}

impl PeerNetwork {
    /// Starts a production endpoint with discovery, NAT traversal and relay fallback.
    pub async fn start() -> Result<Self, NetworkError> {
        let endpoint = Endpoint::bind(presets::N0)
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        Ok(Self::from_endpoint(endpoint))
    }

    pub async fn start_with_secret(secret: [u8; 32]) -> Result<Self, NetworkError> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&secret))
            .bind()
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        Ok(Self::from_endpoint(endpoint))
    }

    /// Starts a local/direct-only endpoint, primarily useful for LAN deployments and tests.
    pub async fn start_direct() -> Result<Self, NetworkError> {
        let endpoint = Endpoint::builder(presets::Minimal)
            .relay_mode(iroh::RelayMode::Disabled)
            .bind()
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        Ok(Self::from_endpoint(endpoint))
    }

    fn from_endpoint(endpoint: Endpoint) -> Self {
        let (sender, receiver) = mpsc::channel(256);
        let (call_sender, call_receiver) = mpsc::channel(64);
        let (media_sender, media_receiver) = mpsc::channel(1024);
        let media_sessions = Arc::new(MediaSessionRegistry::new(
            endpoint.id().to_string(),
            media_sender,
        ));
        let router = Router::builder(endpoint)
            .accept(pptalk_protocol::SYNC_ALPN, EnvelopeHandler { sender })
            .accept(
                pptalk_protocol::CALL_ALPN,
                EnvelopeHandler {
                    sender: call_sender,
                },
            )
            .accept(
                pptalk_protocol::MEDIA_ALPN,
                MediaDatagramHandler {
                    sessions: Arc::clone(&media_sessions),
                },
            )
            .spawn();
        Self {
            router,
            incoming: Arc::new(Mutex::new(receiver)),
            calls: Arc::new(Mutex::new(call_receiver)),
            media: Arc::new(Mutex::new(media_receiver)),
            media_sessions,
        }
    }

    pub fn local_address(&self) -> PeerAddress {
        self.router.endpoint().addr().into()
    }

    pub async fn send(&self, peer: &PeerAddress, bytes: &[u8]) -> Result<(), NetworkError> {
        tokio::time::timeout(
            INTERACTIVE_CONNECT_TIMEOUT,
            self.send_on(peer, pptalk_protocol::SYNC_ALPN, bytes),
        )
        .await
        .map_err(|_| NetworkError::Transport("peer connection timed out".into()))?
    }

    pub async fn send_call(&self, peer: &PeerAddress, bytes: &[u8]) -> Result<(), NetworkError> {
        tokio::time::timeout(
            INTERACTIVE_CONNECT_TIMEOUT,
            self.send_on(peer, pptalk_protocol::CALL_ALPN, bytes),
        )
        .await
        .map_err(|_| NetworkError::Transport("call signaling timed out".into()))?
    }

    async fn send_on(
        &self,
        peer: &PeerAddress,
        alpn: &[u8],
        bytes: &[u8],
    ) -> Result<(), NetworkError> {
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(NetworkError::Transport(
                "envelope exceeds protocol limit".into(),
            ));
        }
        let connection = self
            .router
            .endpoint()
            .connect(peer.to_iroh()?, alpn)
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        let (mut send, mut receive) = connection
            .open_bi()
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        send.write_all(bytes)
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        send.finish()
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        receive
            .read_to_end(0)
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        connection.close(0_u32.into(), b"complete");
        Ok(())
    }

    pub async fn receive(&self) -> Result<IncomingEnvelope, NetworkError> {
        self.incoming
            .lock()
            .await
            .recv()
            .await
            .ok_or(NetworkError::IncomingClosed)
    }

    pub async fn receive_call(&self) -> Result<IncomingEnvelope, NetworkError> {
        self.calls
            .lock()
            .await
            .recv()
            .await
            .ok_or(NetworkError::IncomingClosed)
    }

    pub async fn connect_media(&self, peer: &PeerAddress) -> Result<MediaSession, NetworkError> {
        if let Some(session) = self.media_sessions.get(&peer.endpoint_id).await {
            return Ok(session);
        }

        // Both peers enabling a microphone at the same time used to create two unrelated
        // one-way QUIC connections. Only the accepting side installed a datagram reader,
        // so the usable direction depended on who won the call setup race. Pick one
        // canonical dialer per endpoint pair and make the resulting connection duplex.
        let local_endpoint_id = self.local_address().endpoint_id;
        if local_endpoint_id.as_str() < peer.endpoint_id.as_str() {
            return self.dial_media(peer).await;
        }

        if let Ok(session) = self
            .media_sessions
            .wait_for(&peer.endpoint_id, INTERACTIVE_CONNECT_TIMEOUT)
            .await
        {
            return Ok(session);
        }

        // A bounded fallback keeps media available if the canonical peer is an older
        // client or its incoming connection was lost during path establishment.
        self.dial_media(peer).await
    }

    async fn dial_media(&self, peer: &PeerAddress) -> Result<MediaSession, NetworkError> {
        let connection = tokio::time::timeout(
            INTERACTIVE_CONNECT_TIMEOUT,
            self.router
                .endpoint()
                .connect(peer.to_iroh()?, pptalk_protocol::MEDIA_ALPN),
        )
        .await
        .map_err(|_| NetworkError::Transport("media connection timed out".into()))?
        .map_err(|error| NetworkError::Transport(error.to_string()))?;
        Ok(self.media_sessions.register(connection).await)
    }

    pub async fn close_media(&self, remote_endpoint_id: &str) {
        self.media_sessions.close(remote_endpoint_id).await;
    }

    pub async fn receive_media(&self) -> Result<IncomingMediaDatagram, NetworkError> {
        self.media
            .lock()
            .await
            .recv()
            .await
            .ok_or(NetworkError::IncomingClosed)
    }

    pub async fn shutdown(&self) -> Result<(), NetworkError> {
        self.router
            .shutdown()
            .await
            .map_err(|error| NetworkError::Transport(error.to_string()))
    }
}

#[derive(Debug, Clone)]
struct EnvelopeHandler {
    sender: mpsc::Sender<IncomingEnvelope>,
}

#[derive(Debug, Clone)]
struct MediaDatagramHandler {
    sessions: Arc<MediaSessionRegistry>,
}

impl ProtocolHandler for MediaDatagramHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.sessions.register(connection).await;
        Ok(())
    }
}

#[derive(Debug)]
struct RegisteredMediaSession {
    generation: u64,
    connection: Connection,
}

#[derive(Debug)]
struct MediaSessionRegistry {
    local_endpoint_id: String,
    sessions: Mutex<HashMap<String, RegisteredMediaSession>>,
    incoming: mpsc::Sender<IncomingMediaDatagram>,
    changed: Notify,
    next_generation: AtomicU64,
}

impl MediaSessionRegistry {
    fn new(local_endpoint_id: String, incoming: mpsc::Sender<IncomingMediaDatagram>) -> Self {
        Self {
            local_endpoint_id,
            sessions: Mutex::new(HashMap::new()),
            incoming,
            changed: Notify::new(),
            next_generation: AtomicU64::new(1),
        }
    }

    async fn get(&self, remote_endpoint_id: &str) -> Option<MediaSession> {
        self.sessions
            .lock()
            .await
            .get(remote_endpoint_id)
            .map(|registered| MediaSession {
                connection: registered.connection.clone(),
                remote_endpoint_id: remote_endpoint_id.to_owned(),
            })
    }

    async fn wait_for(
        &self,
        remote_endpoint_id: &str,
        timeout: std::time::Duration,
    ) -> Result<MediaSession, NetworkError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(session) = self.get(remote_endpoint_id).await {
                return Ok(session);
            }
            tokio::time::timeout_at(deadline, notified)
                .await
                .map_err(|_| NetworkError::Transport("media connection timed out".into()))?;
        }
    }

    async fn register(self: &Arc<Self>, connection: Connection) -> MediaSession {
        let remote_endpoint_id = connection.remote_id().to_string();
        let canonical_is_client = self.local_endpoint_id.as_str() < remote_endpoint_id.as_str();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let mut sessions = self.sessions.lock().await;
        if let Some(existing) = sessions.get(&remote_endpoint_id) {
            let existing_is_canonical =
                existing.connection.side().is_client() == canonical_is_client;
            if existing_is_canonical {
                connection.close(0_u32.into(), b"duplicate");
                return MediaSession {
                    connection: existing.connection.clone(),
                    remote_endpoint_id,
                };
            }
        }
        let previous = sessions.insert(
            remote_endpoint_id.clone(),
            RegisteredMediaSession {
                generation,
                connection: connection.clone(),
            },
        );
        drop(sessions);
        if let Some(previous) = previous {
            previous.connection.close(0_u32.into(), b"replaced");
        }
        self.changed.notify_waiters();

        let registry = Arc::clone(self);
        let reader = connection.clone();
        let reader_remote = remote_endpoint_id.clone();
        tokio::spawn(async move {
            while let Ok(bytes) = reader.read_datagram().await {
                if registry
                    .incoming
                    .send(IncomingMediaDatagram {
                        remote_endpoint_id: reader_remote.clone(),
                        bytes: bytes.into(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            registry.remove_if_current(&reader_remote, generation).await;
        });

        MediaSession {
            connection,
            remote_endpoint_id,
        }
    }

    async fn remove_if_current(&self, remote_endpoint_id: &str, generation: u64) {
        let mut sessions = self.sessions.lock().await;
        if sessions
            .get(remote_endpoint_id)
            .is_some_and(|registered| registered.generation == generation)
        {
            sessions.remove(remote_endpoint_id);
        }
    }

    async fn close(&self, remote_endpoint_id: &str) {
        if let Some(session) = self.sessions.lock().await.remove(remote_endpoint_id) {
            session.connection.close(0_u32.into(), b"call complete");
        }
    }
}

impl ProtocolHandler for EnvelopeHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote_endpoint_id = connection.remote_id().to_string();
        let (mut send, mut receive) = connection.accept_bi().await?;
        let bytes = receive
            .read_to_end(MAX_ENVELOPE_BYTES)
            .await
            .map_err(AcceptError::from_err)?;
        self.sender
            .send(IncomingEnvelope {
                remote_endpoint_id,
                bytes,
            })
            .await
            .map_err(|error| AcceptError::from_err(std::io::Error::other(error.to_string())))?;
        send.finish().map_err(AcceptError::from_err)?;
        connection.closed().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transfers_an_envelope_between_direct_endpoints() {
        let alice = PeerNetwork::start_direct().await.expect("alice");
        let bob = PeerNetwork::start_direct().await.expect("bob");
        let bob_address = bob.local_address();

        alice
            .send(&bob_address, b"encrypted event")
            .await
            .expect("send");
        let incoming = bob.receive().await.expect("receive");
        assert_eq!(incoming.bytes, b"encrypted event");
        assert_eq!(
            incoming.remote_endpoint_id,
            alice.local_address().endpoint_id
        );

        alice.shutdown().await.expect("alice shutdown");
        bob.shutdown().await.expect("bob shutdown");
    }

    #[tokio::test]
    async fn call_control_uses_its_own_protocol_channel() {
        use pptalk_protocol::{CallSignal, IdentityId};

        let alice = PeerNetwork::start_direct().await.expect("alice");
        let bob = PeerNetwork::start_direct().await.expect("bob");
        let signal = CallSignal::Invite {
            call_id: pptalk_protocol::CallId::from_bytes([3; 32]),
            selected: vec![IdentityId::from_bytes([4; 32])],
            ring: false,
        };
        alice
            .send_call_signal(&bob.local_address(), &signal)
            .await
            .expect("call signal");
        let (_, received) = bob.receive_call_signal().await.expect("receive signal");
        assert_eq!(received, signal);

        alice.shutdown().await.expect("alice shutdown");
        bob.shutdown().await.expect("bob shutdown");
    }

    #[tokio::test]
    async fn media_datagrams_are_duplex_regardless_of_connect_order() {
        async fn exercise(reverse_connect_order: bool) {
            let alice = PeerNetwork::start_direct().await.expect("alice");
            let bob = PeerNetwork::start_direct().await.expect("bob");
            let alice_address = alice.local_address();
            let bob_address = bob.local_address();

            let (alice_session, bob_session) = if reverse_connect_order {
                let pending = tokio::spawn({
                    let bob = bob.clone();
                    let alice_address = alice_address.clone();
                    async move { bob.connect_media(&alice_address).await }
                });
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let alice_session = alice
                    .connect_media(&bob_address)
                    .await
                    .expect("alice media session");
                let bob_session = pending
                    .await
                    .expect("bob media task")
                    .expect("bob media session");
                (alice_session, bob_session)
            } else {
                let pending = tokio::spawn({
                    let alice = alice.clone();
                    let bob_address = bob_address.clone();
                    async move { alice.connect_media(&bob_address).await }
                });
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let bob_session = bob
                    .connect_media(&alice_address)
                    .await
                    .expect("bob media session");
                let alice_session = pending
                    .await
                    .expect("alice media task")
                    .expect("alice media session");
                (alice_session, bob_session)
            };

            assert_eq!(alice_session.remote_endpoint_id(), bob_address.endpoint_id);
            assert_eq!(bob_session.remote_endpoint_id(), alice_address.endpoint_id);
            alice_session
                .send(vec![0x80, 0x60, 1, 2, 3])
                .await
                .expect("alice datagram");
            bob_session
                .send(vec![0x80, 0x60, 9, 8, 7])
                .await
                .expect("bob datagram");

            let (at_alice, at_bob) = tokio::join!(alice.receive_media(), bob.receive_media());
            let at_alice = at_alice.expect("alice receives bob");
            let at_bob = at_bob.expect("bob receives alice");
            assert_eq!(at_alice.bytes, vec![0x80, 0x60, 9, 8, 7]);
            assert_eq!(at_alice.remote_endpoint_id, bob_address.endpoint_id);
            assert_eq!(at_bob.bytes, vec![0x80, 0x60, 1, 2, 3]);
            assert_eq!(at_bob.remote_endpoint_id, alice_address.endpoint_id);

            alice.close_media(&bob_address.endpoint_id).await;
            bob.close_media(&alice_address.endpoint_id).await;
            alice.shutdown().await.expect("alice shutdown");
            bob.shutdown().await.expect("bob shutdown");
        }

        exercise(false).await;
        exercise(true).await;
    }

    #[tokio::test]
    async fn media_connection_can_be_reestablished_after_close() {
        let alice = PeerNetwork::start_direct().await.expect("alice");
        let bob = PeerNetwork::start_direct().await.expect("bob");
        let alice_address = alice.local_address();
        let bob_address = bob.local_address();

        let (first_alice, first_bob) = tokio::join!(
            alice.connect_media(&bob_address),
            bob.connect_media(&alice_address)
        );
        first_alice.expect("first alice session");
        first_bob.expect("first bob session");
        alice.close_media(&bob_address.endpoint_id).await;
        bob.close_media(&alice_address.endpoint_id).await;

        let (second_alice, second_bob) = tokio::join!(
            alice.connect_media(&bob_address),
            bob.connect_media(&alice_address)
        );
        let second_alice = second_alice.expect("second alice session");
        let second_bob = second_bob.expect("second bob session");
        second_alice
            .send(vec![0x80, 0x60, 1, 2, 3])
            .await
            .expect("datagram");
        second_bob
            .send(vec![0x80, 0x60, 4, 5, 6])
            .await
            .expect("return datagram");
        let (at_alice, at_bob) = tokio::join!(alice.receive_media(), bob.receive_media());
        assert_eq!(
            at_alice.expect("alice packet").bytes,
            vec![0x80, 0x60, 4, 5, 6]
        );
        assert_eq!(at_bob.expect("bob packet").bytes, vec![0x80, 0x60, 1, 2, 3]);

        alice.shutdown().await.expect("alice shutdown");
        bob.shutdown().await.expect("bob shutdown");
    }
}
