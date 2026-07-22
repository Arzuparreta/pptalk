//! Network adapter. The concrete Iroh types do not cross this crate boundary.

pub mod transport;

use pptalk_protocol::{CallSignal, SyncFrame, WireDecode, WireEncode};

pub use transport::{
    IncomingEnvelope, IncomingMediaDatagram, MediaSession, NetworkError, PeerAddress, PeerNetwork,
};

impl PeerNetwork {
    pub async fn send_frame(
        &self,
        peer: &PeerAddress,
        frame: &SyncFrame,
    ) -> Result<(), NetworkError> {
        let bytes = frame
            .to_wire()
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        self.send(peer, &bytes).await
    }

    pub async fn receive_frame(&self) -> Result<(String, SyncFrame), NetworkError> {
        let incoming = self.receive().await?;
        let frame = SyncFrame::from_wire(&incoming.bytes)
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        Ok((incoming.remote_endpoint_id, frame))
    }

    pub async fn send_call_signal(
        &self,
        peer: &PeerAddress,
        signal: &CallSignal,
    ) -> Result<(), NetworkError> {
        let bytes = signal
            .to_wire()
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        self.send_call(peer, &bytes).await
    }

    pub async fn receive_call_signal(&self) -> Result<(String, CallSignal), NetworkError> {
        let incoming = self.receive_call().await?;
        let signal = CallSignal::from_wire(&incoming.bytes)
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        Ok((incoming.remote_endpoint_id, signal))
    }
}
