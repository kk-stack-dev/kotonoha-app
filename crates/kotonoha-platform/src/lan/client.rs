//! Sending work to the paired computer.
//!
//! A connection is only made to a LAN address, and only kept when the computer that answers proves it
//! holds the paired key, wherever it is found. When the last known address fails, the computer is
//! looked up again with mDNS by its id.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kotonoha_core::lan::address;
use kotonoha_core::lan::pairing::{self, hex, PairingCode};
use kotonoha_core::lan::protocol::{self, decode, encode, ErrorKind, Request, Response};
use kotonoha_core::ports::{EngineError, RemoteEngine};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::discovery;
use super::peers::PairedServer;
use super::transport::Channel;
use super::{random, Identity, IdentitySource, LanError, PeerStore};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Longer than whisper.cpp's own limit on the other computer, plus waiting for its previous piece of work.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(300);
const COMPLETE_TIMEOUT: Duration = Duration::from_secs(240);
const INFO_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to listen for mDNS announcements when the last known address fails.
const REDISCOVER_WAIT: Duration = Duration::from_millis(2_500);
/// Pairing waits for the person at the other computer.
const PAIR_ANSWER_TIMEOUT: Duration = Duration::from_secs(130);

async fn within<T>(
    limit: Duration,
    work: impl std::future::Future<Output = Result<T, LanError>>,
) -> Result<T, LanError> {
    timeout(limit, work).await.map_err(|_| LanError::TimedOut)?
}

async fn connect(addr: SocketAddr, identity: &Identity) -> Result<Channel, LanError> {
    if !address::is_lan(addr.ip()) {
        return Err(LanError::NotLan);
    }
    let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| LanError::Connect(std::io::ErrorKind::TimedOut.into()))?
        .map_err(LanError::Connect)?;
    let _ = stream.set_nodelay(true);
    within(HANDSHAKE_TIMEOUT, Channel::connect(stream, identity)).await
}

fn refused(response: Response) -> LanError {
    match response {
        Response::Error { kind, message } => LanError::Refused { kind, message },
        other => LanError::Protocol(format!("unexpected reply: {other:?}")),
    }
}

/// Pairs with the computer at `addr`. `on_code` gets the six digits to show while the person confirms
/// them on the other computer. Returns the paired computer; storing it is up to the caller.
pub async fn pair(
    addr: SocketAddr,
    identity: &Identity,
    my_name: &str,
    on_code: impl FnOnce(PairingCode) + Send,
) -> Result<PairedServer, LanError> {
    let mut channel = connect(addr, identity).await?;
    let client_nonce: pairing::Nonce = random();
    let commit = Request::PairCommit {
        version: protocol::VERSION,
        name: my_name.to_owned(),
        commitment: hex(&pairing::commit(&client_nonce)),
    };
    within(SEND_TIMEOUT, channel.send(&encode(&commit, &[]))).await?;
    let (name, server_nonce) = match decode::<Response>(&within(SEND_TIMEOUT, channel.recv()).await?)?.0 {
        Response::PairNonce { name, nonce } => {
            let nonce = pairing::from_hex::<32>(&nonce).ok_or_else(|| LanError::Protocol("bad nonce".into()))?;
            (protocol::clean_name(&name), nonce)
        }
        other => return Err(refused(other)),
    };
    let reveal = Request::PairReveal { nonce: hex(&client_nonce) };
    within(SEND_TIMEOUT, channel.send(&encode(&reveal, &[]))).await?;
    on_code(PairingCode::new(channel.handshake_hash(), &client_nonce, &server_nonce));
    match decode::<Response>(&within(PAIR_ANSWER_TIMEOUT, channel.recv()).await?)?.0 {
        Response::PairResult { accepted: true } => {
            let key = channel.remote_key();
            Ok(PairedServer { key: hex(key), id: pairing::fingerprint(key), name, address: Some(addr.to_string()) })
        }
        Response::PairResult { accepted: false } => Err(LanError::Declined),
        other => Err(refused(other)),
    }
}

/// The paired computer, as a [`RemoteEngine`] for sessions.
pub struct LanClient {
    identity: Arc<IdentitySource>,
    peers: Arc<PeerStore>,
}

impl LanClient {
    pub fn new(identity: Arc<IdentitySource>, peers: Arc<PeerStore>) -> Self {
        Self { identity, peers }
    }

    /// Connects to the paired computer: its last known address first, then wherever mDNS finds its id.
    async fn open(&self) -> Result<Channel, LanError> {
        let server = self.peers.get().server.ok_or_else(|| LanError::Protocol("no computer is paired".into()))?;
        let identity = self.identity.get()?;
        let mut last_error = LanError::Connect(std::io::ErrorKind::NotFound.into());
        let known: Option<SocketAddr> = server.address.as_deref().and_then(|a| a.parse().ok());
        if let Some(addr) = known {
            match self.try_open(addr, &identity, &server).await {
                Ok(channel) => return Ok(channel),
                Err(e) => last_error = e,
            }
        }
        let own_id = identity.id();
        let found = tokio::task::spawn_blocking(move || discovery::browse(REDISCOVER_WAIT, Some(&own_id)))
            .await
            .map_err(|e| LanError::Discovery(e.to_string()))??;
        let addresses = found.into_iter().filter(|c| c.id == server.id).flat_map(|c| c.addresses);
        for addr in addresses.filter(|a| Some(*a) != known) {
            match self.try_open(addr, &identity, &server).await {
                Ok(channel) => {
                    tracing::info!("paired computer found at a new address");
                    let address = Some(addr.to_string());
                    let _ = self.peers.update(|p| {
                        if let Some(s) = p.server.as_mut().filter(|s| s.key == server.key) {
                            s.address = address;
                        }
                    });
                    return Ok(channel);
                }
                Err(e) => last_error = e,
            }
        }
        Err(last_error)
    }

    async fn try_open(
        &self,
        addr: SocketAddr,
        identity: &Identity,
        server: &PairedServer,
    ) -> Result<Channel, LanError> {
        let channel = connect(addr, identity).await?;
        if hex(channel.remote_key()) != server.key {
            return Err(LanError::WrongPeer);
        }
        Ok(channel)
    }

    async fn exchange(&self, request: &Request, body: &[u8], limit: Duration) -> Result<Response, EngineError> {
        let mut channel = self.open().await.map_err(|e| EngineError::Unreachable(e.to_string()))?;
        let lost = |e: LanError| EngineError::Unreachable(e.to_string());
        within(SEND_TIMEOUT, channel.send(&encode(request, body))).await.map_err(lost)?;
        let reply = match within(limit, channel.recv()).await {
            Ok(reply) => reply,
            Err(LanError::TimedOut) => return Err(EngineError::TimedOut),
            Err(e) => return Err(lost(e)),
        };
        let (response, _) = decode::<Response>(&reply).map_err(|e| EngineError::Failed(e.to_string()))?;
        match response {
            Response::Error { kind, message } => Err(engine_error(kind, message)),
            response => Ok(response),
        }
    }

    /// Asks the paired computer what it can do. For the settings page's connection check.
    pub async fn check(&self) -> Result<(bool, Option<String>), LanError> {
        let mut channel = self.open().await?;
        within(SEND_TIMEOUT, channel.send(&encode(&Request::Info { version: protocol::VERSION }, &[]))).await?;
        match decode::<Response>(&within(INFO_TIMEOUT, channel.recv()).await?)?.0 {
            Response::Info { ready, llm_model } => Ok((ready, llm_model)),
            other => Err(refused(other)),
        }
    }
}

fn engine_error(kind: ErrorKind, message: String) -> EngineError {
    match kind {
        ErrorKind::TimedOut => EngineError::TimedOut,
        kind => EngineError::Failed(format!("the paired computer refused ({kind:?}): {message}")),
    }
}

fn expect_text(response: Response) -> Result<String, EngineError> {
    match response {
        Response::Text { text } => Ok(text),
        other => Err(EngineError::Failed(format!("unexpected reply: {other:?}"))),
    }
}

#[async_trait]
impl RemoteEngine for LanClient {
    fn is_paired(&self) -> bool {
        self.peers.get().server.is_some()
    }

    async fn transcribe(&self, samples: &[f32], language: &str, prompt: &str) -> Result<String, EngineError> {
        let request = Request::Transcribe {
            version: protocol::VERSION,
            language: language.to_owned(),
            prompt: prompt.to_owned(),
        };
        expect_text(self.exchange(&request, &protocol::samples_to_pcm16(samples), TRANSCRIBE_TIMEOUT).await?)
    }

    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String, EngineError> {
        let request = Request::Complete { version: protocol::VERSION, prompt: prompt.to_owned(), max_tokens };
        expect_text(self.exchange(&request, &[], COMPLETE_TIMEOUT).await?)
    }

    async fn llm_model(&self) -> Result<Option<String>, EngineError> {
        match self.exchange(&Request::Info { version: protocol::VERSION }, &[], INFO_TIMEOUT).await? {
            Response::Info { llm_model, .. } => Ok(llm_model),
            other => Err(EngineError::Failed(format!("unexpected reply: {other:?}"))),
        }
    }
}
