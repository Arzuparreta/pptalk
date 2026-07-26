use std::{net::SocketAddr, str::FromStr, sync::Arc};

use iroh::{
    Endpoint, EndpointAddr, PublicKey, RelayUrl, SecretKey, TransportAddr,
    endpoint::{Connection, presets},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use pptalk_protocol::MAX_ENVELOPE_BYTES;
use tokio::sync::{Mutex, mpsc};

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
                    sender: media_sender,
                },
            )
            .spawn();
        Self {
            router,
            incoming: Arc::new(Mutex::new(receiver)),
            calls: Arc::new(Mutex::new(call_receiver)),
            media: Arc::new(Mutex::new(media_receiver)),
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
        let connection = tokio::time::timeout(
            INTERACTIVE_CONNECT_TIMEOUT,
            self.router
                .endpoint()
                .connect(peer.to_iroh()?, pptalk_protocol::MEDIA_ALPN),
        )
        .await
        .map_err(|_| NetworkError::Transport("media connection timed out".into()))?
        .map_err(|error| NetworkError::Transport(error.to_string()))?;
        Ok(MediaSession { connection })
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
    sender: mpsc::Sender<IncomingMediaDatagram>,
}

impl ProtocolHandler for MediaDatagramHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote_endpoint_id = connection.remote_id().to_string();
        while let Ok(bytes) = connection.read_datagram().await {
            if self
                .sender
                .send(IncomingMediaDatagram {
                    remote_endpoint_id: remote_endpoint_id.clone(),
                    bytes: bytes.into(),
                })
                .await
                .is_err()
            {
                break;
            }
        }
        Ok(())
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
    async fn media_datagrams_cross_a_latency_sensitive_channel() {
        let alice = PeerNetwork::start_direct().await.expect("alice");
        let bob = PeerNetwork::start_direct().await.expect("bob");
        let session = alice
            .connect_media(&bob.local_address())
            .await
            .expect("media session");
        session
            .send(vec![0x80, 0x60, 1, 2, 3])
            .await
            .expect("datagram");
        let packet = bob.receive_media().await.expect("media packet");
        assert_eq!(packet.bytes, vec![0x80, 0x60, 1, 2, 3]);
        assert_eq!(packet.remote_endpoint_id, alice.local_address().endpoint_id);

        alice.shutdown().await.expect("alice shutdown");
        bob.shutdown().await.expect("bob shutdown");
    }
}
