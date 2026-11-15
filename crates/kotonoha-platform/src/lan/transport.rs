//! An encrypted, mutually authenticated channel over TCP (Noise XX: X25519, ChaCha20-Poly1305,
//! BLAKE2s). Both sides learn the other's long-term public key during the handshake.
//!
//! On the wire every Noise message is `[length: u16 BE][ciphertext]`. A protocol message is sent as
//! one Noise message holding its length (u32 BE), then as many Noise messages as its bytes need.

use kotonoha_core::lan::protocol::MAX_MESSAGE_BYTES;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{Identity, LanError};

pub(crate) const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const MAX_NOISE: usize = 65_535;
const TAG: usize = 16;

pub(crate) struct Channel {
    stream: TcpStream,
    noise: snow::TransportState,
    remote_key: [u8; 32],
    handshake_hash: Vec<u8>,
}

impl Channel {
    /// The connecting side.
    pub async fn connect(mut stream: TcpStream, identity: &Identity) -> Result<Self, LanError> {
        let mut hs = builder(identity)?.build_initiator()?;
        let mut buf = vec![0; MAX_NOISE];
        // -> e
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(&mut stream, &buf[..n]).await?;
        // <- e, ee, s, es
        let frame = read_frame(&mut stream).await?;
        hs.read_message(&frame, &mut buf)?;
        // -> s, se
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(&mut stream, &buf[..n]).await?;
        Self::finish(stream, hs)
    }

    /// The accepting side.
    pub async fn accept(mut stream: TcpStream, identity: &Identity) -> Result<Self, LanError> {
        let mut hs = builder(identity)?.build_responder()?;
        let mut buf = vec![0; MAX_NOISE];
        let frame = read_frame(&mut stream).await?;
        hs.read_message(&frame, &mut buf)?;
        let n = hs.write_message(&[], &mut buf)?;
        write_frame(&mut stream, &buf[..n]).await?;
        let frame = read_frame(&mut stream).await?;
        hs.read_message(&frame, &mut buf)?;
        Self::finish(stream, hs)
    }

    fn finish(stream: TcpStream, hs: snow::HandshakeState) -> Result<Self, LanError> {
        let remote_key: [u8; 32] = hs
            .get_remote_static()
            .and_then(|k| k.try_into().ok())
            .ok_or_else(|| LanError::Protocol("no remote key".into()))?;
        let handshake_hash = hs.get_handshake_hash().to_vec();
        Ok(Self { stream, noise: hs.into_transport_mode()?, remote_key, handshake_hash })
    }

    /// The other computer's long-term public key, proven by the handshake.
    pub fn remote_key(&self) -> &[u8; 32] {
        &self.remote_key
    }

    /// Unique to this connection; both sides see the same value.
    pub fn handshake_hash(&self) -> &[u8] {
        &self.handshake_hash
    }

    /// Resolves once the other side has closed the connection. Waits forever if it sends data instead.
    pub async fn peek_closed(&mut self, buf: &mut [u8; 1]) {
        match self.stream.peek(buf).await {
            Ok(0) | Err(_) => {}
            Ok(_) => std::future::pending().await,
        }
    }

    pub async fn send(&mut self, message: &[u8]) -> Result<(), LanError> {
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(LanError::TooLarge);
        }
        let mut buf = vec![0; MAX_NOISE];
        let n = self.noise.write_message(&(message.len() as u32).to_be_bytes(), &mut buf)?;
        write_frame(&mut self.stream, &buf[..n]).await?;
        for part in message.chunks(MAX_NOISE - TAG) {
            let n = self.noise.write_message(part, &mut buf)?;
            write_frame(&mut self.stream, &buf[..n]).await?;
        }
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>, LanError> {
        let mut buf = vec![0; MAX_NOISE];
        let frame = read_frame(&mut self.stream).await?;
        let n = self.noise.read_message(&frame, &mut buf)?;
        let len: [u8; 4] = buf[..n].try_into().map_err(|_| LanError::Protocol("bad length".into()))?;
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_MESSAGE_BYTES {
            return Err(LanError::TooLarge);
        }
        let mut message = Vec::with_capacity(len);
        while message.len() < len {
            let frame = read_frame(&mut self.stream).await?;
            let n = self.noise.read_message(&frame, &mut buf)?;
            message.extend_from_slice(&buf[..n]);
        }
        if message.len() != len {
            return Err(LanError::Protocol("length mismatch".into()));
        }
        Ok(message)
    }
}

fn builder(identity: &Identity) -> Result<snow::Builder<'_>, LanError> {
    let params = NOISE_PARAMS.parse().expect("valid Noise parameters");
    Ok(snow::Builder::new(params).local_private_key(identity.private())?)
}

async fn write_frame(stream: &mut TcpStream, frame: &[u8]) -> Result<(), LanError> {
    stream.write_all(&(frame.len() as u16).to_be_bytes()).await?;
    stream.write_all(frame).await?;
    Ok(())
}

async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, LanError> {
    let mut len = [0; 2];
    stream.read_exact(&mut len).await?;
    let mut frame = vec![0; usize::from(u16::from_be_bytes(len))];
    stream.read_exact(&mut frame).await?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn both_sides_learn_keys_and_exchange_large_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_id = Identity::generate().unwrap();
        let client_id = Identity::generate().unwrap();
        let server_public = *server_id.public();
        let client_public = *client_id.public();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ch = Channel::accept(stream, &server_id).await.unwrap();
            assert_eq!(ch.remote_key(), &client_public);
            let message = ch.recv().await.unwrap();
            ch.send(&message).await.unwrap();
            ch.handshake_hash().to_vec()
        });
        let stream = TcpStream::connect(addr).await.unwrap();
        let mut ch = Channel::connect(stream, &client_id).await.unwrap();
        assert_eq!(ch.remote_key(), &server_public);
        let big: Vec<u8> = (0..300_000).map(|i| (i % 251) as u8).collect();
        ch.send(&big).await.unwrap();
        assert_eq!(ch.recv().await.unwrap(), big);
        assert_eq!(server.await.unwrap(), ch.handshake_hash());
    }
}
