//! Noise-encrypted, length-framed message channel over any byte stream (TCP in practice).
//!
//! Handshake: `Noise_XX_25519_ChaChaPoly_BLAKE2s` with prologue [`crate::PROLOGUE`]. XX
//! transmits both static keys (encrypted), so each side learns the peer's key and then
//! decides whether it trusts it: the caller compares [`Established::remote`] with its
//! pinned key(s). Nothing application-level is sent before that check.
//!
//! After the handshake the stream is split: a [`Sender`] and a [`Receiver`] that can be
//! used from different tasks. Each direction has its own nonce counter (snow's stateless
//! transport), so no lock is needed. Frame: `u16` big-endian length + Noise message.

use std::fmt;
use std::io;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use snow::StatelessTransportState;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::keys::{Identity, PublicKey};
use crate::{MAX_FRAME, MAX_PAYLOAD, NOISE_PARAMS, PROLOGUE};

#[derive(Debug)]
pub enum ChannelError {
    Io(io::Error),
    /// The Noise handshake failed (wrong protocol, tampering, not a Pocket peer).
    Handshake(String),
    /// A frame did not decrypt (tampering, desynchronised peer).
    Decrypt,
    /// A message exceeded [`MAX_PAYLOAD`].
    TooLarge(usize),
    /// A frame decrypted but is not a valid message.
    Decode(serde_json::Error),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelError::Io(e) => write!(f, "{e}"),
            ChannelError::Handshake(e) => write!(f, "handshake failed: {e}"),
            ChannelError::Decrypt => f.write_str("frame failed authentication"),
            ChannelError::TooLarge(n) => write!(f, "message of {n} bytes is too large"),
            ChannelError::Decode(e) => write!(f, "invalid message: {e}"),
        }
    }
}

impl std::error::Error for ChannelError {}

impl From<io::Error> for ChannelError {
    fn from(e: io::Error) -> Self {
        ChannelError::Io(e)
    }
}

fn hs_err(e: impl fmt::Display) -> ChannelError {
    ChannelError::Handshake(e.to_string())
}

/// A completed handshake: the two halves and the peer's authenticated static key.
pub struct Established<S> {
    pub sender: Sender<S>,
    pub receiver: Receiver<S>,
    /// The peer's static key. Authenticated by the handshake, but **not yet trusted**:
    /// the caller must compare it with its pinned key(s).
    pub remote: PublicKey,
}

fn builder(identity: &Identity) -> Result<snow::Builder<'_>, ChannelError> {
    snow::Builder::new(NOISE_PARAMS.parse().map_err(hs_err)?)
        .local_private_key(identity.private_bytes())
        .map_err(hs_err)?
        .prologue(PROLOGUE)
        .map_err(hs_err)
}

/// Runs the handshake as the initiator (the phone).
pub async fn initiate<S>(mut stream: S, identity: &Identity) -> Result<Established<S>, ChannelError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut hs = builder(identity)?.build_initiator().map_err(hs_err)?;
    let mut buf = vec![0u8; MAX_FRAME];
    let mut scratch = vec![0u8; MAX_FRAME];
    // -> e
    let n = hs.write_message(&[], &mut buf).map_err(hs_err)?;
    write_frame(&mut stream, &buf[..n]).await?;
    // <- e, ee, s, es
    let frame = read_frame(&mut stream)
        .await?
        .ok_or_else(|| hs_err("peer closed"))?;
    hs.read_message(&frame, &mut scratch).map_err(hs_err)?;
    // -> s, se
    let n = hs.write_message(&[], &mut buf).map_err(hs_err)?;
    write_frame(&mut stream, &buf[..n]).await?;
    finish(stream, hs)
}

/// Runs the handshake as the responder (the desktop agent).
pub async fn respond<S>(mut stream: S, identity: &Identity) -> Result<Established<S>, ChannelError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut hs = builder(identity)?.build_responder().map_err(hs_err)?;
    let mut buf = vec![0u8; MAX_FRAME];
    let mut scratch = vec![0u8; MAX_FRAME];
    let frame = read_frame(&mut stream)
        .await?
        .ok_or_else(|| hs_err("peer closed"))?;
    hs.read_message(&frame, &mut scratch).map_err(hs_err)?;
    let n = hs.write_message(&[], &mut buf).map_err(hs_err)?;
    write_frame(&mut stream, &buf[..n]).await?;
    let frame = read_frame(&mut stream)
        .await?
        .ok_or_else(|| hs_err("peer closed"))?;
    hs.read_message(&frame, &mut scratch).map_err(hs_err)?;
    finish(stream, hs)
}

fn finish<S>(stream: S, hs: snow::HandshakeState) -> Result<Established<S>, ChannelError>
where
    S: AsyncRead + AsyncWrite,
{
    let remote = hs
        .get_remote_static()
        .and_then(PublicKey::from_slice)
        .ok_or_else(|| hs_err("no remote static key"))?;
    let state = Arc::new(hs.into_stateless_transport_mode().map_err(hs_err)?);
    let (r, w) = tokio::io::split(stream);
    Ok(Established {
        sender: Sender {
            half: w,
            state: state.clone(),
            nonce: 0,
            buf: vec![0u8; MAX_FRAME],
        },
        receiver: Receiver {
            half: r,
            state,
            nonce: 0,
            buf: vec![0u8; MAX_FRAME],
        },
        remote,
    })
}

/// Sending half: serialises, encrypts and frames one message per call.
pub struct Sender<S> {
    half: WriteHalf<S>,
    state: Arc<StatelessTransportState>,
    nonce: u64,
    buf: Vec<u8>,
}

impl<S: AsyncWrite> Sender<S> {
    pub async fn send<T: Serialize>(&mut self, msg: &T) -> Result<(), ChannelError> {
        let json = serde_json::to_vec(msg).map_err(ChannelError::Decode)?;
        if json.len() > MAX_PAYLOAD {
            return Err(ChannelError::TooLarge(json.len()));
        }
        let n = self
            .state
            .write_message(self.nonce, &json, &mut self.buf)
            .map_err(hs_err)?;
        self.nonce += 1;
        write_frame(&mut self.half, &self.buf[..n]).await
    }

    /// Flushes and half-closes the write side (the peer sees EOF).
    pub async fn close(&mut self) {
        let _ = self.half.shutdown().await;
    }
}

/// Receiving half. Not cancel-safe: drive it from one task (e.g. a reader loop that
/// forwards messages into a channel) rather than from a `select!` branch.
pub struct Receiver<S> {
    half: ReadHalf<S>,
    state: Arc<StatelessTransportState>,
    nonce: u64,
    buf: Vec<u8>,
}

impl<S: AsyncRead> Receiver<S> {
    /// Next message, or `None` when the peer closed the stream cleanly between frames.
    pub async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, ChannelError> {
        let Some(frame) = read_frame(&mut self.half).await? else {
            return Ok(None);
        };
        let n = self
            .state
            .read_message(self.nonce, &frame, &mut self.buf)
            .map_err(|_| ChannelError::Decrypt)?;
        self.nonce += 1;
        serde_json::from_slice(&self.buf[..n])
            .map(Some)
            .map_err(ChannelError::Decode)
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, data: &[u8]) -> Result<(), ChannelError> {
    let len = u16::try_from(data.len()).map_err(|_| ChannelError::TooLarge(data.len()))?;
    let mut out = Vec::with_capacity(2 + data.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    w.write_all(&out).await?;
    w.flush().await?;
    Ok(())
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<u8>>, ChannelError> {
    let mut len = [0u8; 2];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = usize::from(u16::from_be_bytes(len));
    if len == 0 {
        return Err(ChannelError::Handshake("empty frame".into()));
    }
    let mut frame = vec![0u8; len];
    r.read_exact(&mut frame).await?;
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{AgentMessage, ClientMessage};

    #[tokio::test]
    async fn handshake_authenticates_both_static_keys_and_carries_messages() {
        let (a, b) = tokio::io::duplex(4096);
        let phone = Identity::generate().unwrap();
        let desk = Identity::generate().unwrap();
        let (p, d) = tokio::join!(initiate(a, &phone), respond(b, &desk));
        let (mut p, mut d) = (p.unwrap(), d.unwrap());
        assert_eq!(p.remote, desk.public());
        assert_eq!(d.remote, phone.public());

        for id in 0..3 {
            p.sender.send(&ClientMessage::Ping { id }).await.unwrap();
            let got: ClientMessage = d.receiver.recv().await.unwrap().unwrap();
            assert_eq!(got, ClientMessage::Ping { id });
            d.sender.send(&AgentMessage::Pong { id }).await.unwrap();
            let got: AgentMessage = p.receiver.recv().await.unwrap().unwrap();
            assert_eq!(got, AgentMessage::Pong { id });
        }
        drop(d);
        assert!(p.receiver.recv::<AgentMessage>().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn non_noise_peer_fails_the_handshake() {
        let (a, mut b) = tokio::io::duplex(4096);
        let phone = Identity::generate().unwrap();
        let peer = tokio::spawn(async move {
            let mut junk = [0u8; 64];
            let _ = b.read(&mut junk).await;
            b.write_all(b"\x00\x05hello").await.unwrap();
            b
        });
        assert!(matches!(
            initiate(a, &phone).await,
            Err(ChannelError::Handshake(_))
        ));
        drop(peer.await);
    }

    #[tokio::test]
    async fn tampered_frame_is_rejected() {
        let (a, b) = tokio::io::duplex(4096);
        let (x, y) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let (p, d) = tokio::join!(initiate(a, &x), respond(b, &y));
        let (p, mut d) = (p.unwrap(), d.unwrap());
        // Encrypt with a skipped nonce: authentication of the frame must fail.
        let mut s = p.sender;
        s.nonce = 5;
        s.send(&ClientMessage::Ping { id: 1 }).await.unwrap();
        assert!(matches!(
            d.receiver.recv::<ClientMessage>().await,
            Err(ChannelError::Decrypt)
        ));
    }
}
