//! Accepting work from paired computers on the LAN.
//!
//! Only connections from LAN addresses are answered. Work is only done for paired computers, one piece
//! at a time. Pairing needs the person at this computer to approve the code. Logs carry sizes and
//! timings, never the transcribed text.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use kotonoha_core::lan::address;
use kotonoha_core::lan::pairing::{self, from_hex, hex, PairingCode};
use kotonoha_core::lan::protocol::{self, decode, encode, ErrorKind, Request, Response};
use kotonoha_core::ports::{CompletionRequest, EngineError, LanguageModel, SpeechRecognizer, TranscriptionRequest};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

use super::peers::PairedClient;
use super::transport::Channel;
use super::{random, Identity, LanError, PeerStore};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the person has to answer a pairing request.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CONNECTIONS: usize = 8;

/// The models this computer would use right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChosenModels {
    pub whisper: Option<PathBuf>,
    pub llm: Option<PathBuf>,
}

/// The work this computer does for others.
#[async_trait]
pub trait LocalWork: Send + Sync {
    fn models(&self) -> ChosenModels;
    async fn transcribe(&self, samples: &[f32], language: &str, prompt: &str) -> Result<String, EngineError>;
    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String, EngineError>;
}

/// whisper.cpp and llama.cpp with the models chosen by `choose`, as for this computer's own sessions.
pub struct LocalEngines {
    pub recognizer: Arc<dyn SpeechRecognizer>,
    pub llm: Arc<dyn LanguageModel>,
    pub choose: Box<dyn Fn() -> ChosenModels + Send + Sync>,
}

#[async_trait]
impl LocalWork for LocalEngines {
    fn models(&self) -> ChosenModels {
        (self.choose)()
    }

    async fn transcribe(&self, samples: &[f32], language: &str, prompt: &str) -> Result<String, EngineError> {
        let model = self.models().whisper.ok_or_else(|| EngineError::ModelMissing(PathBuf::new()))?;
        self.recognizer.transcribe(TranscriptionRequest { samples, model: &model, language, prompt }).await
    }

    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String, EngineError> {
        let model = self.models().llm.ok_or_else(|| EngineError::ModelMissing(PathBuf::new()))?;
        self.llm.complete(CompletionRequest { model: &model, prompt, max_tokens }).await
    }
}

/// A computer asking to pair, shown to the person with the code.
#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub name: String,
    pub code: PairingCode,
}

#[async_trait]
pub trait PairingApprover: Send + Sync {
    /// Shows the request and returns the person's answer. Abandoned (and taken as "no") after two minutes.
    async fn approve(&self, request: PairingRequest) -> bool;
    /// The request was abandoned before an answer (the other computer gave up, or time ran out).
    fn withdraw(&self) {}
}

pub struct ServerConfig {
    pub identity: Arc<Identity>,
    pub peers: Arc<PeerStore>,
    /// This computer's name, shown to computers that pair with it.
    pub name: String,
    pub work: Arc<dyn LocalWork>,
    pub approver: Arc<dyn PairingApprover>,
    /// Normally 0.0.0.0 on the default port.
    pub bind: SocketAddr,
}

/// A running server. Dropping it stops accepting and ends every open connection.
pub struct Server {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Server {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        tracing::info!("stopped accepting work");
    }
}

struct Shared {
    config: ServerConfig,
    /// One piece of work at a time: whisper.cpp already uses every core.
    busy: Mutex<()>,
    pairing: AtomicBool,
}

/// Binds the port and starts accepting. Binding is what makes Windows ask about the firewall.
pub async fn start(config: ServerConfig) -> Result<Server, LanError> {
    let listener = TcpListener::bind(config.bind).await?;
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "accepting work from paired computers");
    let shared = Arc::new(Shared { config, busy: Mutex::new(()), pairing: AtomicBool::new(false) });
    let task = tokio::spawn(accept_loop(listener, shared));
    Ok(Server { addr, task })
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    // Dropping the set (when the server stops) aborts every connection
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    if !address::is_lan(peer.ip()) {
                        tracing::warn!(%peer, "refused a connection from outside the LAN");
                        continue;
                    }
                    if connections.len() >= MAX_CONNECTIONS {
                        tracing::warn!(%peer, "refused a connection: too many open");
                        continue;
                    }
                    let shared = shared.clone();
                    connections.spawn(async move {
                        if let Err(e) = handle(stream, peer, &shared).await {
                            tracing::info!(%peer, error = %e, "connection ended with an error");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            },
            Some(_) = connections.join_next() => {}
        }
    }
}

async fn within<T>(
    limit: Duration,
    work: impl std::future::Future<Output = Result<T, LanError>>,
) -> Result<T, LanError> {
    timeout(limit, work).await.map_err(|_| LanError::TimedOut)?
}

async fn reply(channel: &mut Channel, response: &Response) -> Result<(), LanError> {
    within(REQUEST_TIMEOUT, channel.send(&encode(response, &[]))).await
}

fn error(kind: ErrorKind, message: impl Into<String>) -> Response {
    Response::Error { kind, message: message.into() }
}

async fn handle(stream: TcpStream, peer: SocketAddr, shared: &Shared) -> Result<(), LanError> {
    let config = &shared.config;
    let mut channel = within(HANDSHAKE_TIMEOUT, Channel::accept(stream, &config.identity)).await?;
    let message = within(REQUEST_TIMEOUT, channel.recv()).await?;
    let (request, body) = decode::<Request>(&message)?;
    let client = hex(channel.remote_key());

    let version = match &request {
        Request::PairCommit { version, .. }
        | Request::Info { version }
        | Request::Transcribe { version, .. }
        | Request::Complete { version, .. } => *version,
        Request::PairReveal { .. } => return reply(&mut channel, &error(ErrorKind::BadRequest, "out of order")).await,
    };
    if version != protocol::VERSION {
        return reply(&mut channel, &error(ErrorKind::Version, format!("version {}", protocol::VERSION))).await;
    }
    if let Request::PairCommit { name, commitment, .. } = request {
        return pair(&mut channel, shared, protocol::clean_name(&name), &commitment).await;
    }
    if !config.peers.is_client(&client) {
        tracing::warn!(%peer, "refused work from a computer that is not paired");
        return reply(&mut channel, &error(ErrorKind::NotPaired, "not paired")).await;
    }

    let response = match request {
        Request::Info { .. } => {
            let models = config.work.models();
            Response::Info {
                ready: models.whisper.is_some(),
                llm_model: models.llm.and_then(|m| m.file_name().map(|n| n.to_string_lossy().into_owned())),
            }
        }
        Request::Transcribe { language, prompt, .. } => {
            let samples = protocol::pcm16_to_samples(body);
            let _turn = shared.busy.lock().await;
            let started = Instant::now();
            let result = config.work.transcribe(&samples, &language, &prompt).await;
            tracing::info!(
                %peer,
                audio_secs = samples.len() as f64 / 16_000.0,
                took_secs = started.elapsed().as_secs_f64(),
                ok = result.is_ok(),
                chars = result.as_ref().map_or(0, |t| t.chars().count()),
                "transcribed for a paired computer"
            );
            work_response(result)
        }
        Request::Complete { prompt, max_tokens, .. } => {
            let _turn = shared.busy.lock().await;
            let started = Instant::now();
            let result = config.work.complete(&prompt, max_tokens.min(4096)).await;
            tracing::info!(
                %peer,
                took_secs = started.elapsed().as_secs_f64(),
                ok = result.is_ok(),
                "cleaned up text for a paired computer"
            );
            work_response(result)
        }
        Request::PairCommit { .. } | Request::PairReveal { .. } => unreachable!("handled above"),
    };
    reply(&mut channel, &response).await
}

fn work_response(result: Result<String, EngineError>) -> Response {
    match result {
        Ok(text) => Response::Text { text },
        Err(e @ (EngineError::ToolMissing(_) | EngineError::ModelMissing(_))) => {
            error(ErrorKind::Unavailable, e.to_string())
        }
        Err(EngineError::TimedOut) => error(ErrorKind::TimedOut, "timed out"),
        Err(e) => error(ErrorKind::Failed, e.to_string()),
    }
}

/// Clears the "pairing in progress" flag however the pairing ends.
struct PairingSlot<'a>(&'a AtomicBool);

impl Drop for PairingSlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn pair(channel: &mut Channel, shared: &Shared, name: String, commitment: &str) -> Result<(), LanError> {
    let config = &shared.config;
    let Some(commitment) = from_hex::<32>(commitment) else {
        return reply(channel, &error(ErrorKind::BadRequest, "bad commitment")).await;
    };
    if shared.pairing.swap(true, Ordering::SeqCst) {
        return reply(channel, &error(ErrorKind::Failed, "another pairing is in progress")).await;
    }
    let _slot = PairingSlot(&shared.pairing);

    let server_nonce: pairing::Nonce = random();
    reply(channel, &Response::PairNonce { name: config.name.clone(), nonce: hex(&server_nonce) }).await?;
    let message = within(REQUEST_TIMEOUT, channel.recv()).await?;
    let Request::PairReveal { nonce } = decode::<Request>(&message)?.0 else {
        return reply(channel, &error(ErrorKind::BadRequest, "expected the nonce")).await;
    };
    let Some(client_nonce) = from_hex::<32>(&nonce).filter(|n| pairing::verify(&commitment, n)) else {
        return reply(channel, &error(ErrorKind::BadRequest, "nonce does not match")).await;
    };
    let code = PairingCode::new(channel.handshake_hash(), &client_nonce, &server_nonce);

    // Asks the person, giving up when time runs out or the other computer hangs up
    let mut closed = [0u8; 1];
    let accepted = tokio::select! {
        answer = timeout(APPROVAL_TIMEOUT, config.approver.approve(PairingRequest { name: name.clone(), code })) => {
            answer.unwrap_or(false)
        }
        _ = wait_closed(channel, &mut closed) => {
            config.approver.withdraw();
            tracing::info!("pairing abandoned by the other computer");
            return Ok(());
        }
    };
    if !accepted {
        config.approver.withdraw();
    }
    if accepted {
        config
            .peers
            .add_client(PairedClient { key: hex(channel.remote_key()), name })
            .map_err(|e| LanError::Key(e.to_string()))?;
    }
    tracing::info!(accepted, "pairing answered");
    reply(channel, &Response::PairResult { accepted }).await
}

/// Resolves when the other side closes the connection (it sends nothing while waiting for the answer).
async fn wait_closed(channel: &mut Channel, buf: &mut [u8; 1]) {
    channel.peek_closed(buf).await;
}
