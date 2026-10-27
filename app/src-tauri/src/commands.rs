//! Commands the settings and overlay pages call. Errors are returned as user-facing Japanese text.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use kotonoha_core::dictionary::{Dictionary, EditError, Entry};
use kotonoha_core::models::{self, CatalogModel, MemoryMode, ModelFile, ModelKind, ModelTier, CATALOG};
use kotonoha_core::ports::{ConfigSource, SystemProbe};
use kotonoha_core::session::{Phase, SessionHandle};
use kotonoha_core::settings::Settings;
use kotonoha_core::setup::{self, Download, Plan, Present, Progress};
use kotonoha_core::shortcut::{paste_label, Platform, Shortcut};
use kotonoha_core::tools::{Target, TOOL_CATALOG};
use kotonoha_platform::audio::{self, MicPermission};
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::system::MemoryProbe;
use kotonoha_platform::tools::{self, Tool};
use kotonoha_platform::{download, library, output};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt;

use crate::{shortcut, tray, windows};

type Store<'a> = State<'a, Arc<ConfigStore>>;

/// Strings the pages need to describe keys the way this OS names them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiContext {
    platform: Platform,
    shortcut_label: String,
    paste_label: String,
    version: &'static str,
}

#[tauri::command]
pub fn ui_context(store: Store<'_>) -> UiContext {
    let platform = Platform::current();
    UiContext {
        platform,
        shortcut_label: store.settings().shortcut.label(platform),
        paste_label: paste_label(platform),
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
pub fn current_phase(session: State<'_, SessionHandle>) -> Phase {
    session.phase().borrow().clone()
}

#[tauri::command]
pub fn toggle_recording(session: State<'_, SessionHandle>) {
    session.toggle();
}

#[tauri::command]
pub fn get_settings(store: Store<'_>) -> Settings {
    store.settings()
}

/// Saves settings. A new shortcut is registered first; if the OS refuses it (another app owns it),
/// the old one is restored and nothing is saved.
#[tauri::command]
pub fn save_settings<R: Runtime>(app: AppHandle<R>, store: Store<'_>, settings: Settings) -> Result<(), String> {
    settings.shortcut.validate().map_err(|e| format!("ショートカットが不正です: {e}"))?;
    let previous = store.settings();
    if settings.shortcut != previous.shortcut {
        if let Err(e) = shortcut::register(&app, &settings.shortcut) {
            let _ = shortcut::register(&app, &previous.shortcut);
            return Err(format!("このショートカットは登録できません（他のアプリが使っている可能性があります）: {e}"));
        }
    }
    let label = settings.shortcut.label(Platform::current());
    store.save_settings(settings).map_err(|e| format!("設定を保存できません: {e}"))?;
    tray::set_tooltip(&app, &tooltip(&label));
    let _ = app.emit("settings-changed", ());
    Ok(())
}

#[tauri::command]
pub fn shortcut_label(shortcut: Shortcut) -> Result<String, String> {
    shortcut.validate().map_err(|e| e.to_string())?;
    Ok(shortcut.label(Platform::current()))
}

#[tauri::command]
pub fn suspend_shortcut<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    shortcut::suspend(&app)
}

#[tauri::command]
pub fn resume_shortcut<R: Runtime>(app: AppHandle<R>, store: Store<'_>) -> Result<(), String> {
    shortcut::register(&app, &store.settings().shortcut)
}

#[tauri::command]
pub fn get_dictionary(store: Store<'_>) -> Dictionary {
    store.dictionary()
}

/// `variants` is free text such as "クロード、Claud".
#[tauri::command]
pub fn add_word(store: Store<'_>, word: String, variants: String) -> Result<Dictionary, String> {
    let added = store
        .update_dictionary(|d| d.add(&word, Entry::parse_variants(&variants)))
        .map_err(|e| format!("辞書を保存できません: {e}"))?;
    if !added {
        return Err("空の語か、すでに登録されている語です".into());
    }
    Ok(store.dictionary())
}

#[tauri::command]
pub fn remove_word(store: Store<'_>, word: String) -> Result<Dictionary, String> {
    store.update_dictionary(|d| d.remove(&word)).map_err(|e| format!("辞書を保存できません: {e}"))?;
    Ok(store.dictionary())
}

#[tauri::command]
pub fn edit_word(store: Store<'_>, word: String, new_word: String, variants: String) -> Result<Dictionary, String> {
    store
        .update_dictionary(|d| d.edit(&word, &new_word, Entry::parse_variants(&variants)))
        .map_err(|e| format!("辞書を保存できません: {e}"))?
        .map_err(|e| match e {
            EditError::Empty => "表記を入力してください".to_owned(),
            EditError::Duplicate => "すでに登録されている語です".to_owned(),
            EditError::NotFound => "この語は見つかりません".to_owned(),
            EditError::Builtin => "標準の言葉は変更できません".to_owned(),
        })?;
    Ok(store.dictionary())
}

/// Turns a word on or off without removing it. Shipped words can only be turned off this way.
#[tauri::command]
pub fn set_word_enabled(store: Store<'_>, word: String, enabled: bool) -> Result<Dictionary, String> {
    store.update_dictionary(|d| d.set_enabled(&word, enabled)).map_err(|e| format!("辞書を保存できません: {e}"))?;
    Ok(store.dictionary())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    #[serde(flatten)]
    model: CatalogModel,
    present: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    whisper_tool: Option<PathBuf>,
    llama_tool: Option<PathBuf>,
    models_dir: PathBuf,
    models: Vec<ModelFile>,
    /// What "automatic" picks right now, from free memory.
    auto_whisper: Option<String>,
    auto_llm: Option<String>,
    catalog: Vec<CatalogEntry>,
    /// What setup still has to download (or ask the user to install) for this machine.
    plan: Plan,
    plan_bytes: u64,
    /// Dictation works: a speech recognition tool and model are in place.
    ready: bool,
    input_permission: bool,
    microphone_permission: MicPermission,
    memory_mode: MemoryMode,
    available_memory_mb: u64,
}

fn locate_tools(store: &ConfigStore) -> (Option<PathBuf>, Option<PathBuf>) {
    let settings = store.settings();
    let own = store.dirs().tools_dir();
    (
        tools::locate(Tool::Whisper, settings.tools.whisper.as_deref(), Some(&own)),
        tools::locate(Tool::Llama, settings.tools.llama.as_deref(), Some(&own)),
    )
}

fn current_plan(store: &ConfigStore, mode: MemoryMode, library: &[ModelFile]) -> Plan {
    let (whisper_tool, llama_tool) = locate_tools(store);
    let has = |kind| library.iter().any(|m| m.kind == kind);
    let present = Present {
        whisper_tool: whisper_tool.is_some(),
        llama_tool: llama_tool.is_some(),
        whisper_model: has(ModelKind::Whisper),
        llm_model: has(ModelKind::Llm),
    };
    setup::plan(mode, Target::current(), present, TOOL_CATALOG)
}

#[tauri::command]
pub fn setup_status(store: Store<'_>) -> SetupStatus {
    let models_dir = store.models_dir();
    let available = MemoryProbe::default().available_memory_mb();
    let mode = MemoryMode::from_available_mb(available);
    let library = library::scan(&models_dir);
    let (whisper_tool, llama_tool) = locate_tools(&store);
    let plan = current_plan(&store, mode, &library);
    SetupStatus {
        ready: whisper_tool.is_some() && library.iter().any(|m| m.kind == ModelKind::Whisper),
        whisper_tool,
        llama_tool,
        auto_whisper: models::choose_whisper(&library, mode).map(|m| m.name.clone()),
        auto_llm: models::choose_llm(&library, mode).map(|m| m.name.clone()),
        models: library,
        catalog: CATALOG
            .iter()
            .map(|m| CatalogEntry { model: *m, present: models_dir.join(m.file).is_file() })
            .collect(),
        plan_bytes: plan.total_bytes(),
        plan,
        models_dir,
        input_permission: output::has_input_permission(),
        microphone_permission: audio::microphone_permission(),
        memory_mode: mode,
        available_memory_mb: available,
    }
}

/// Set while a download runs; cancelling sets `cancel`, which the download checks between reads.
#[derive(Default)]
pub struct Downloads {
    running: AtomicBool,
    cancel: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    #[serde(flatten)]
    progress: Progress,
    purpose: ModelKind,
}

/// What `start_download` fetches: everything setup still needs, or the light models for the 詳細 tab.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadSet {
    Setup,
    LightModels,
}

/// Downloads a set, reporting overall progress as `download-progress` events. Errors are user-facing;
/// a cancelled download returns the error "cancelled", which the pages treat as a quiet stop.
#[tauri::command]
pub async fn start_download<R: Runtime>(
    app: AppHandle<R>,
    store: Store<'_>,
    downloads: State<'_, Arc<Downloads>>,
    set: DownloadSet,
) -> Result<(), String> {
    if downloads.running.swap(true, Ordering::SeqCst) {
        return Err("ダウンロード中です".into());
    }
    downloads.cancel.store(false, Ordering::SeqCst);
    let models_dir = store.models_dir();
    let items: Vec<Download> = match set {
        DownloadSet::Setup => {
            let mode = MemoryMode::from_available_mb(MemoryProbe::default().available_memory_mb());
            current_plan(&store, mode, &library::scan(&models_dir)).downloads
        }
        DownloadSet::LightModels => CATALOG
            .iter()
            .filter(|m| m.tier == ModelTier::Light && !models_dir.join(m.file).is_file())
            .map(Download::model)
            .collect(),
    };
    let tools_dir = store.dirs().tools_dir();
    let state = downloads.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let folders = download::Folders { tools: &tools_dir, models: &models_dir };
        let total = items.iter().map(|d| d.bytes).sum();
        let mut finished = 0;
        for (index, item) in items.iter().enumerate() {
            let mut last_percent = u64::MAX;
            let report = |current| {
                let progress = Progress::new(index, items.len(), finished, current, total);
                (progress, DownloadProgress { progress, purpose: item.purpose })
            };
            let _ = app.emit("download-progress", report(0).1);
            download::fetch(item, &folders, &state.cancel, |done, _| {
                let (progress, event) = report(done);
                if progress.percent() != last_percent {
                    last_percent = progress.percent();
                    let _ = app.emit("download-progress", event);
                }
            })
            .map_err(|e| match e {
                download::DownloadError::Cancelled => "cancelled".to_owned(),
                download::DownloadError::Http(_) => {
                    "ダウンロードできませんでした。インターネットの接続を確かめて、もう一度お試しください".to_owned()
                }
                other => format!("{} を保存できませんでした: {other}", item.file),
            })?;
            finished += item.bytes;
            tracing::info!(file = %item.file, "downloaded");
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    downloads.running.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn cancel_download(downloads: State<'_, Arc<Downloads>>) {
    downloads.cancel.store(true, Ordering::SeqCst);
}

/// Shows the macOS prompt if the user has not been asked yet, and returns the answer.
#[tauri::command]
pub async fn request_microphone() -> MicPermission {
    tauri::async_runtime::spawn_blocking(|| audio::request_microphone_permission(std::time::Duration::from_secs(60)))
        .await
        .unwrap_or(MicPermission::NotDetermined)
}

#[tauri::command]
pub fn open_microphone_settings<R: Runtime>(app: AppHandle<R>) {
    let url = match Platform::current() {
        Platform::Windows => "ms-settings:privacy-microphone",
        _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        tracing::warn!(error = %e, "could not open microphone settings");
    }
}

#[tauri::command]
pub fn open_models_folder<R: Runtime>(app: AppHandle<R>, store: Store<'_>) {
    let dir = store.models_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = app.opener().open_path(dir.to_string_lossy(), None::<&str>) {
        tracing::warn!(error = %e, "could not open models folder");
    }
}

#[tauri::command]
pub fn open_setup<R: Runtime>(app: AppHandle<R>) {
    windows::open_setup(&app);
}

/// Closes the setup window. Settings stay reachable from the tray.
#[tauri::command]
pub fn finish_setup<R: Runtime>(app: AppHandle<R>) {
    windows::close_setup(&app);
}

#[tauri::command]
pub fn open_input_permission_settings() {
    output::open_input_permission_settings();
}

#[tauri::command]
pub fn open_logs<R: Runtime>(app: AppHandle<R>) {
    open_logs_folder(&app);
}

pub fn open_logs_folder<R: Runtime>(app: &AppHandle<R>) {
    let Some(store) = app.try_state::<Arc<ConfigStore>>() else { return };
    let dir = store.dirs().logs.clone();
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = app.opener().open_path(dir.to_string_lossy(), None::<&str>) {
        tracing::warn!(error = %e, "could not open log folder");
    }
}

#[tauri::command]
pub fn get_autostart<R: Runtime>(app: AppHandle<R>) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if enabled { manager.enable() } else { manager.disable() };
    result.map_err(|e| format!("自動起動を変更できません: {e}"))
}

pub fn tooltip(shortcut_label: &str) -> String {
    format!("Kotonoha - {shortcut_label} で録音")
}
