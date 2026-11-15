//! Two computers on one machine, over loopback: pairing, work for a paired computer, and refusals.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use kotonoha_core::lan::pairing::PairingCode;
use kotonoha_core::ports::{EngineError, RemoteEngine};
use kotonoha_platform::lan::{
    self, ChosenModels, Identity, IdentitySource, LanClient, LanError, LocalWork, PairingApprover, PairingRequest,
    PeerStore, Server, ServerConfig,
};

struct Echo;

#[async_trait]
impl LocalWork for Echo {
    fn models(&self) -> ChosenModels {
        ChosenModels { whisper: Some(PathBuf::from("/m/ggml-small.bin")), llm: Some(PathBuf::from("/m/qwen.gguf")) }
    }
    async fn transcribe(&self, samples: &[f32], language: &str, prompt: &str) -> Result<String, EngineError> {
        Ok(format!("{} {language} {prompt}", samples.len()))
    }
    async fn complete(&self, prompt: &str, max_tokens: usize) -> Result<String, EngineError> {
        Ok(format!("{prompt}/{max_tokens}"))
    }
}

/// Answers every request with `accept`, and remembers the codes it was shown.
struct Approver {
    accept: bool,
    shown: Mutex<Vec<PairingCode>>,
}

#[async_trait]
impl PairingApprover for Approver {
    async fn approve(&self, request: PairingRequest) -> bool {
        assert_eq!(request.name, "client-pc");
        self.shown.lock().unwrap().push(request.code);
        self.accept
    }
}

struct Machine {
    _dir: tempfile::TempDir,
    identity: Arc<IdentitySource>,
    peers: Arc<PeerStore>,
}

fn machine() -> Machine {
    let dir = tempfile::tempdir().unwrap();
    let identity = Arc::new(IdentitySource::new(dir.path().join("lan-key")));
    let peers = Arc::new(PeerStore::open(dir.path().join("lan-peers.json")));
    Machine { _dir: dir, identity, peers }
}

async fn serve(machine: &Machine, approver: Arc<Approver>) -> Server {
    lan::start(ServerConfig {
        identity: machine.identity.get().unwrap(),
        peers: machine.peers.clone(),
        name: "server-pc".into(),
        work: Arc::new(Echo),
        approver,
        bind: "127.0.0.1:0".parse().unwrap(),
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn pairs_then_sends_work() {
    let server_pc = machine();
    let approver = Arc::new(Approver { accept: true, shown: Mutex::default() });
    let server = serve(&server_pc, approver.clone()).await;

    let client_pc = machine();
    let client = LanClient::new(client_pc.identity.clone(), client_pc.peers.clone());
    assert!(!client.is_paired());

    let mut seen = None;
    let paired =
        lan::pair(server.local_addr(), &client_pc.identity.get().unwrap(), "client-pc", |code| seen = Some(code))
            .await
            .unwrap();
    assert_eq!(paired.name, "server-pc");
    assert_eq!(paired.key, kotonoha_core::lan::pairing::hex(server_pc.identity.get().unwrap().public()));
    // Both screens showed the same code
    assert_eq!(approver.shown.lock().unwrap().as_slice(), [seen.unwrap()]);
    assert_eq!(server_pc.peers.get().clients.len(), 1);
    assert_eq!(server_pc.peers.get().clients[0].name, "client-pc");

    client_pc.peers.update(|p| p.server = Some(paired)).unwrap();
    assert!(client.is_paired());
    assert_eq!(client.transcribe(&[0.1; 16_000], "ja", "Claude、").await.unwrap(), "16000 ja Claude、");
    assert_eq!(client.complete("整えて", 128).await.unwrap(), "整えて/128");
    assert_eq!(client.llm_model().await.unwrap().as_deref(), Some("qwen.gguf"));
    assert_eq!(client.check().await.unwrap(), (true, Some("qwen.gguf".into())));
}

#[tokio::test]
async fn declined_pairing_stores_nothing() {
    let server_pc = machine();
    let server = serve(&server_pc, Arc::new(Approver { accept: false, shown: Mutex::default() })).await;
    let client_pc = machine();
    let result = lan::pair(server.local_addr(), &client_pc.identity.get().unwrap(), "client-pc", |_| {}).await;
    assert!(matches!(result, Err(LanError::Declined)));
    assert!(server_pc.peers.get().clients.is_empty());
}

#[tokio::test]
async fn unpaired_and_impostor_computers_are_refused() {
    let server_pc = machine();
    let server = serve(&server_pc, Arc::new(Approver { accept: true, shown: Mutex::default() })).await;
    let server_key = kotonoha_core::lan::pairing::hex(server_pc.identity.get().unwrap().public());

    // A client that believes it is paired, but the server never approved it
    let stranger = machine();
    stranger
        .peers
        .update(|p| {
            p.server = Some(lan::PairedServer {
                key: server_key.clone(),
                id: "x".into(),
                name: "server-pc".into(),
                address: Some(server.local_addr().to_string()),
            })
        })
        .unwrap();
    let client = LanClient::new(stranger.identity.clone(), stranger.peers.clone());
    let refused = client.transcribe(&[0.0; 10], "ja", "").await.unwrap_err();
    assert!(matches!(&refused, EngineError::Failed(m) if m.contains("NotPaired")), "{refused:?}");

    // A different computer at the paired computer's address is not trusted with audio
    let other_key = kotonoha_core::lan::pairing::hex(Identity::generate().unwrap().public());
    stranger.peers.update(|p| p.server.as_mut().unwrap().key = other_key).unwrap();
    let wrong = client.transcribe(&[0.0; 10], "ja", "").await.unwrap_err();
    assert!(matches!(wrong, EngineError::Unreachable(_)), "{wrong:?}");
}

#[tokio::test]
async fn stopped_server_is_unreachable() {
    let server_pc = machine();
    let server = serve(&server_pc, Arc::new(Approver { accept: true, shown: Mutex::default() })).await;
    let addr = server.local_addr();
    let client_pc = machine();
    let paired = lan::pair(addr, &client_pc.identity.get().unwrap(), "client-pc", |_| {}).await.unwrap();
    client_pc.peers.update(|p| p.server = Some(paired)).unwrap();
    drop(server);

    let client = LanClient::new(client_pc.identity.clone(), client_pc.peers.clone());
    let error = client.transcribe(&[0.0; 10], "ja", "").await.unwrap_err();
    assert!(matches!(error, EngineError::Unreachable(_)), "{error:?}");
}

#[tokio::test]
async fn addresses_outside_the_lan_are_never_dialled() {
    let client_pc = machine();
    let result = lan::pair("8.8.8.8:50540".parse().unwrap(), &client_pc.identity.get().unwrap(), "c", |_| {}).await;
    assert!(matches!(result, Err(LanError::NotLan)));
}
