//! Processing on another computer in the same LAN: the settings page's commands, the server this
//! computer runs while it accepts work, and the pairing prompts.
//!
//! Nothing here touches the network until the person turns a LAN feature on in settings (or turned
//! it on earlier and the app starts again). That first use is when the OS asks for its permission.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use kotonoha_core::lan::protocol::ErrorKind;
use kotonoha_core::lan::{address, DEFAULT_PORT};
use kotonoha_core::models::{self, MemoryMode};
use kotonoha_core::ports::{ConfigSource, ModelLibrary, SystemProbe};
use kotonoha_core::shortcut::Platform;
use kotonoha_platform::lan::{
    self, Advertisement, ChosenModels, FoundComputer, IdentitySource, LanClient, LanError, LocalEngines, PairedClient,
    PairedServer, PairingApprover, PairingRequest, PeerStore, Server, ServerConfig,
};
use kotonoha_platform::library::ModelDirectory;
use kotonoha_platform::llama::LlamaCli;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::system::MemoryProbe;
use kotonoha_platform::whisper::WhisperCli;
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{oneshot, Mutex};

use crate::windows;

/// How long the settings page listens for computers that accept work.
const BROWSE_WAIT: Duration = Duration::from_secs(3);
/// How often the network category is checked while accepting work (Windows marks public networks).
const NETWORK_CHECK_INTERVAL: Duration = Duration::from_secs(30);

struct Running {
    _server: Server,
    _advertisement: Option<Advertisement>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServeProblem {
    /// Stopped because this computer is on a network Windows marks as public.
    PublicNetwork,
    PortInUse,
    Failed,
}

struct PendingPair {
    name: String,
    code: String,
    answer: oneshot::Sender<bool>,
}

pub struct Lan {
    pub identity: Arc<IdentitySource>,
    pub peers: Arc<PeerStore>,
    store: Arc<ConfigStore>,
    running: Mutex<Option<Running>>,
    problem: std::sync::Mutex<Option<ServeProblem>>,
    pending: std::sync::Mutex<Option<PendingPair>>,
    pairing: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Lan {
    pub fn new(store: Arc<ConfigStore>) -> Self {
        let dirs = store.dirs();
        Self {
            identity: Arc::new(IdentitySource::new(dirs.lan_key_file())),
            peers: Arc::new(PeerStore::open(dirs.lan_peers_file())),
            store,
            running: Mutex::new(None),
            problem: std::sync::Mutex::new(None),
            pending: std::sync::Mutex::new(None),
            pairing: std::sync::Mutex::new(None),
        }
    }

    pub fn client(&self) -> LanClient {
        LanClient::new(self.identity.clone(), self.peers.clone())
    }

    /// A computer is paired and work goes to it.
    pub fn sends_work(&self) -> bool {
        self.store.settings().lan.use_remote && self.peers.get().server.is_some()
    }

    fn set_problem(&self, problem: Option<ServeProblem>) {
        *self.problem.lock().unwrap() = problem;
    }

    async fn start_serving<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>) -> Result<(), ServeProblem> {
        let mut running = self.running.lock().await;
        if running.is_some() {
            return Ok(());
        }
        if lan::network::on_public_network() {
            tracing::warn!("not accepting work: this computer is on a public network");
            return Err(ServeProblem::PublicNetwork);
        }
        let identity = self.identity.get().map_err(|e| {
            tracing::error!(error = %e, "no LAN key");
            ServeProblem::Failed
        })?;
        let name = lan::computer_name();
        let config = ServerConfig {
            identity: identity.clone(),
            peers: self.peers.clone(),
            name: name.clone(),
            work: Arc::new(local_engines(self.store.clone())),
            approver: Arc::new(Approver { app: app.clone(), lan: self.clone() }),
            bind: SocketAddr::from((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT)),
        };
        let server = lan::start(config).await.map_err(|e| {
            tracing::error!(error = %e, "could not start accepting work");
            match e {
                LanError::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => ServeProblem::PortInUse,
                _ => ServeProblem::Failed,
            }
        })?;
        // Without the announcement, other computers can still connect by address
        let advertisement = lan::advertise(&name, &identity.id(), server.local_addr().port())
            .inspect_err(|e| tracing::warn!(error = %e, "could not announce on the LAN"))
            .ok();
        *running = Some(Running { _server: server, _advertisement: advertisement });
        Ok(())
    }

    async fn stop_serving(&self) {
        self.running.lock().await.take();
        // A request still on screen can no longer be answered
        if self.pending.lock().unwrap().take().is_some() {
            tracing::info!("pairing request dropped: stopped accepting work");
        }
    }

    async fn is_serving(&self) -> bool {
        self.running.lock().await.is_some()
    }

    /// Starts serving if the setting is on. Called at launch and by the network watcher.
    pub async fn apply_setting<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>) {
        if !self.store.settings().lan.serve {
            self.stop_serving().await;
            self.set_problem(None);
            return;
        }
        let result = self.start_serving(app).await;
        self.set_problem(result.err());
    }
}

/// Models chosen the same way as for this computer's own sessions.
fn local_engines(store: Arc<ConfigStore>) -> LocalEngines {
    let settings = store.settings();
    let tools = Some(store.dirs().tools_dir());
    LocalEngines {
        recognizer: Arc::new(WhisperCli::new(settings.tools.whisper.clone(), tools.clone())),
        llm: Arc::new(LlamaCli::new(settings.tools.llama.clone(), tools)),
        choose: Box::new(move || {
            let settings = store.settings();
            let dir = store.models_dir();
            let library = ModelDirectory::new(move || dir.clone()).models();
            let mode = MemoryMode::from_available_mb(MemoryProbe::default().available_memory_mb());
            let whisper = settings
                .models
                .whisper
                .clone()
                .or_else(|| models::choose_whisper(&library, mode).map(|m| m.path.clone()));
            let llm = (mode != MemoryMode::Critical)
                .then(|| {
                    settings.models.llm.clone().or_else(|| models::choose_llm(&library, mode).map(|m| m.path.clone()))
                })
                .flatten();
            ChosenModels { whisper, llm }
        }),
    }
}

/// Starts accepting work at launch if it was on, and stops while on a public network.
pub fn spawn<R: Runtime>(app: AppHandle<R>, lan: Arc<Lan>) {
    tauri::async_runtime::spawn(async move {
        lan.apply_setting(&app).await;
        loop {
            tokio::time::sleep(NETWORK_CHECK_INTERVAL).await;
            if !lan.store.settings().lan.serve {
                continue;
            }
            let public = lan::network::on_public_network();
            let serving = lan.is_serving().await;
            if public && serving {
                tracing::warn!("stopped accepting work: joined a public network");
                lan.stop_serving().await;
                lan.set_problem(Some(ServeProblem::PublicNetwork));
                let _ = app.emit("lan-changed", ());
            } else if !public && !serving && *lan.problem.lock().unwrap() == Some(ServeProblem::PublicNetwork) {
                lan.apply_setting(&app).await;
                let _ = app.emit("lan-changed", ());
            }
        }
    });
}

struct Approver<R: Runtime> {
    app: AppHandle<R>,
    lan: Arc<Lan>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairPrompt {
    name: String,
    code: String,
}

#[async_trait::async_trait]
impl<R: Runtime> PairingApprover for Approver<R> {
    async fn approve(&self, request: PairingRequest) -> bool {
        let (answer, answered) = oneshot::channel();
        let prompt = PairPrompt { name: request.name.clone(), code: request.code.to_string() };
        *self.lan.pending.lock().unwrap() =
            Some(PendingPair { name: prompt.name.clone(), code: prompt.code.clone(), answer });
        windows::open_settings(&self.app);
        let _ = self.app.emit("lan-pair-request", Some(prompt));
        let accepted = answered.await.unwrap_or(false);
        if accepted {
            let _ = self.app.emit("lan-changed", ());
        }
        accepted
    }

    fn withdraw(&self) {
        self.lan.pending.lock().unwrap().take();
        let _ = self.app.emit("lan-pair-request", None::<PairPrompt>);
    }
}

// ---------------------------------------------------------------- commands

type LanState<'a> = State<'a, Arc<Lan>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanStatus {
    computer_name: String,
    /// This computer's LAN addresses, shown for typing on the other computer.
    addresses: Vec<String>,
    serving: bool,
    problem: Option<ServeProblem>,
    server: Option<PairedServer>,
    clients: Vec<PairedClient>,
    pair_request: Option<PairPrompt>,
}

#[tauri::command]
pub async fn lan_status(lan: LanState<'_>) -> Result<LanStatus, String> {
    let peers = lan.peers.get();
    let pair_request =
        lan.pending.lock().unwrap().as_ref().map(|p| PairPrompt { name: p.name.clone(), code: p.code.clone() });
    Ok(LanStatus {
        computer_name: lan::computer_name(),
        addresses: lan::local_addresses().iter().map(ToString::to_string).collect(),
        serving: lan.is_serving().await,
        problem: *lan.problem.lock().unwrap(),
        server: peers.server,
        clients: peers.clients,
        pair_request,
    })
}

/// Turns accepting work on or off and saves the choice. Turning it on opens the port and announces this
/// computer, which is when the OS asks for its permission.
#[tauri::command]
pub async fn lan_set_serving<R: Runtime>(
    app: AppHandle<R>,
    lan: LanState<'_>,
    enabled: bool,
) -> Result<LanStatus, String> {
    let mut settings = lan.store.settings();
    settings.lan.serve = enabled;
    lan.store.save_settings(settings).map_err(|e| format!("設定を保存できませんでした: {e}"))?;
    lan.inner().apply_setting(&app).await;
    let _ = app.emit("settings-changed", ());
    lan_status(lan).await
}

/// Lists computers that accept work, other than this one. The first call is what makes macOS ask for
/// the local network permission.
#[tauri::command]
pub async fn lan_browse(lan: LanState<'_>) -> Result<Vec<FoundComputer>, String> {
    let own_id = lan.identity.get().map_err(|e| format!("鍵を用意できませんでした: {e}"))?.id();
    tauri::async_runtime::spawn_blocking(move || lan::browse(BROWSE_WAIT, Some(&own_id)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| {
            tracing::warn!(error = %e, "LAN discovery failed");
            "LAN 内のコンピューターを探せませんでした。ネットワークへの接続を確かめてください".to_owned()
        })
}

/// Pairs with the computer at one of `addresses` (from discovery, or typed by the person). The code
/// arrives as a `lan-pair-code` event while the person confirms it on the other computer.
#[tauri::command]
pub async fn lan_pair<R: Runtime>(
    app: AppHandle<R>,
    lan: LanState<'_>,
    addresses: Vec<String>,
) -> Result<PairedServer, String> {
    let mut targets = Vec::new();
    for text in &addresses {
        match address::parse(text) {
            Ok(addr) => targets.push(addr),
            Err(address::AddressError::NotLan) => return Err(lan_error_text(&LanError::NotLan)),
            Err(address::AddressError::Invalid) => {
                return Err("IP アドレスの形が正しくありません（例: 192.168.1.20）".into());
            }
        }
    }
    if targets.is_empty() {
        return Err("接続先がありません".into());
    }
    let identity = lan.identity.get().map_err(|e| format!("鍵を用意できませんでした: {e}"))?;
    let (done, result) = oneshot::channel();
    let emitter = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let name = lan::computer_name();
        let mut outcome = Err(LanError::Connect(std::io::ErrorKind::NotFound.into()));
        for addr in targets {
            let app = emitter.clone();
            outcome = lan::pair(addr, &identity, &name, move |code| {
                let _ = app.emit("lan-pair-code", code.to_string());
            })
            .await;
            // Another address of the same computer is only worth trying when this one did not answer
            if !matches!(outcome, Err(LanError::Connect(_))) {
                break;
            }
        }
        let _ = done.send(outcome);
    });
    if let Some(previous) = lan.pairing.lock().unwrap().replace(task) {
        previous.abort();
    }
    let outcome = result.await.map_err(|_| "cancelled".to_owned())?;
    lan.pairing.lock().unwrap().take();
    let server = outcome.map_err(|e| {
        tracing::info!(error = %e, "pairing failed");
        lan_error_text(&e)
    })?;
    lan.peers.update(|p| p.server = Some(server.clone())).map_err(|e| format!("保存できませんでした: {e}"))?;
    tracing::info!("paired with a computer on the LAN");
    let _ = app.emit("lan-changed", ());
    Ok(server)
}

#[tauri::command]
pub fn lan_pair_cancel(lan: LanState<'_>) {
    if let Some(task) = lan.pairing.lock().unwrap().take() {
        task.abort();
    }
}

/// The person's answer to a pairing request shown on this computer.
#[tauri::command]
pub fn lan_pair_answer<R: Runtime>(app: AppHandle<R>, lan: LanState<'_>, accept: bool) {
    if let Some(pending) = lan.pending.lock().unwrap().take() {
        let _ = pending.answer.send(accept);
    }
    let _ = app.emit("lan-pair-request", None::<PairPrompt>);
}

#[tauri::command]
pub fn lan_forget_server<R: Runtime>(app: AppHandle<R>, lan: LanState<'_>) -> Result<(), String> {
    lan.peers.update(|p| p.server = None).map_err(|e| format!("保存できませんでした: {e}"))?;
    let _ = app.emit("lan-changed", ());
    Ok(())
}

#[tauri::command]
pub fn lan_forget_client<R: Runtime>(app: AppHandle<R>, lan: LanState<'_>, key: String) -> Result<(), String> {
    lan.peers.update(|p| p.clients.retain(|c| c.key != key)).map_err(|e| format!("保存できませんでした: {e}"))?;
    let _ = app.emit("lan-changed", ());
    Ok(())
}

/// Connects to the paired computer and reports what it can do.
#[tauri::command]
pub async fn lan_check(lan: LanState<'_>) -> Result<String, String> {
    let (ready, llm) = lan.client().check().await.map_err(|e| lan_error_text(&e))?;
    Ok(match (ready, llm) {
        (false, _) => "接続できましたが、相手のコンピューターに音声認識のモデルがありません".into(),
        (true, Some(_)) => "接続できました。音声認識と文章校正を相手のコンピューターで行います".into(),
        (true, None) => {
            "接続できました。音声認識を相手のコンピューターで行います（文章校正のモデルはありません）".into()
        }
    })
}

#[tauri::command]
pub fn open_local_network_settings<R: Runtime>(app: AppHandle<R>) {
    let url = match Platform::current() {
        Platform::Windows => "ms-settings:network-status",
        _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork",
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        tracing::warn!(error = %e, "could not open network settings");
    }
}

fn lan_error_text(e: &LanError) -> String {
    if e.may_be_local_network_permission() {
        return "接続できませんでした。ローカルネットワークへのアクセスが許可されていない可能性があります（システム設定 > プライバシーとセキュリティ > ローカルネットワーク）".into();
    }
    match e {
        LanError::Connect(_) => {
            "接続できませんでした。相手のコンピューターで「ペアリングしたコンピューターの処理を受け付ける」がオンになっているか確かめてください".into()
        }
        LanError::TimedOut => "相手のコンピューターから応答がありませんでした".into(),
        LanError::Declined => "相手のコンピューターで許可されませんでした".into(),
        LanError::NotLan => "LAN の外のアドレスには接続できません".into(),
        LanError::WrongPeer => "ペアリングしたコンピューターとは別のコンピューターが応答しました".into(),
        LanError::Refused { kind: ErrorKind::Version, .. } => {
            "相手の Kotonoha Voice とバージョンが合いません。両方を最新にしてください".into()
        }
        LanError::Refused { kind: ErrorKind::NotPaired, .. } => {
            "相手のコンピューターでペアリングが解除されています。ペアリングし直してください".into()
        }
        LanError::Refused { kind: ErrorKind::Failed, message } if message.contains("pairing") => {
            "相手のコンピューターで、別のペアリングを確認しているところです。終わってからやり直してください".into()
        }
        other => format!("接続に失敗しました: {other}"),
    }
}
