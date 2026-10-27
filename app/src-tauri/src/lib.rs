//! Kotonoha desktop app: the composition root. It wires the core session to the platform adapters,
//! and adds the tray, the global shortcut, the recording overlay and the settings window.

mod bridge;
mod commands;
mod logging;
mod shortcut;
mod tray;
mod windows;

use std::sync::Arc;

use kotonoha_core::models;
use kotonoha_core::ports::{ConfigSource, ModelLibrary, SystemProbe};
use kotonoha_core::session::{self, Services, SessionHandle, Timing};
use kotonoha_core::shortcut::Platform;
use kotonoha_core::tools::Tool;
use kotonoha_platform::audio::CpalMicrophone;
use kotonoha_platform::library::ModelDirectory;
use kotonoha_platform::llama::LlamaCli;
use kotonoha_platform::output::PasteSink;
use kotonoha_platform::paths::AppDirs;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::system::MemoryProbe;
use kotonoha_platform::whisper::WhisperCli;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    let dirs = AppDirs::discover().expect("no home directory");
    let _log_guard = logging::init(&dirs.logs);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Kotonoha starting");
    let store = Arc::new(ConfigStore::open(dirs));

    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch just opens the settings of the running one
        .plugin(tauri_plugin_single_instance::init(|app, _, _| windows::open_settings(app)))
        .plugin(shortcut::plugin())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_opener::init())
        .manage(store.clone())
        .manage(Arc::new(commands::Downloads::default()))
        .on_window_event(|window, event| {
            // The settings page frees the shortcut while recording a new one; if the window closes
            // mid-capture, put the saved shortcut back
            let captures = [windows::SETTINGS, windows::SETUP].contains(&window.label());
            if captures && matches!(event, WindowEvent::Destroyed) {
                let app = window.app_handle();
                if let Some(store) = app.try_state::<Arc<ConfigStore>>() {
                    let _ = shortcut::register(app, &store.settings().shortcut);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::ui_context,
            commands::current_phase,
            commands::toggle_recording,
            commands::get_settings,
            commands::save_settings,
            commands::shortcut_label,
            commands::suspend_shortcut,
            commands::resume_shortcut,
            commands::get_dictionary,
            commands::add_word,
            commands::remove_word,
            commands::edit_word,
            commands::set_word_enabled,
            commands::setup_status,
            commands::start_download,
            commands::cancel_download,
            commands::request_microphone,
            commands::open_microphone_settings,
            commands::open_models_folder,
            commands::open_setup,
            commands::finish_setup,
            commands::open_input_permission_settings,
            commands::open_logs,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .setup(move |app| {
            // A menu bar app: no Dock icon, and never activated by showing the overlay
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            let session = start_session(store.clone());
            app.manage(session.clone());
            windows::create_overlay(handle)?;
            let label = store.settings().shortcut.label(Platform::current());
            tray::create(handle, &commands::tooltip(&label))?;
            bridge::spawn(handle.clone(), session);

            let shortcut_failed = shortcut::register(handle, &store.settings().shortcut)
                .inspect_err(|e| tracing::warn!(error = %e, "could not register the shortcut"))
                .is_err();
            if !can_dictate(&store) {
                // Setup asks for the microphone itself, at the step that explains why
                windows::open_setup(handle);
            } else {
                // Ask at launch rather than on the first (then failing) recording
                std::thread::spawn(|| {
                    let _ =
                        kotonoha_platform::audio::request_microphone_permission(std::time::Duration::from_secs(120));
                });
                if shortcut_failed {
                    windows::open_settings(handle);
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|_, event| {
        // Closing the settings window must not quit a tray app; only "Quit" (an explicit exit code) does
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}

fn start_session(store: Arc<ConfigStore>) -> SessionHandle {
    let settings = store.settings();
    // whisper.cpp and llama.cpp builds downloaded during setup
    let own_tools = Some(store.dirs().tools_dir());
    let models_store = store.clone();
    let services = Services {
        microphone: Arc::new(CpalMicrophone),
        recognizer: Arc::new(WhisperCli::new(settings.tools.whisper.clone(), own_tools.clone())),
        llm: Arc::new(LlamaCli::new(settings.tools.llama.clone(), own_tools)),
        sink: Arc::new(PasteSink),
        probe: Arc::new(MemoryProbe::default()),
        models: Arc::new(ModelDirectory::new(move || models_store.models_dir())),
        config: store,
    };
    // The controller spawns onto Tokio, so start it inside Tauri's runtime
    tauri::async_runtime::block_on(async move { session::spawn(services, Timing::default()) })
}

/// A speech recognition tool and model are in place; otherwise first-run setup opens.
fn can_dictate(store: &ConfigStore) -> bool {
    let dir = store.models_dir();
    let library = ModelDirectory::new(move || dir.clone()).models();
    let mode = models::MemoryMode::from_available_mb(MemoryProbe::default().available_memory_mb());
    let has_model = store.settings().models.whisper.is_some() || models::choose_whisper(&library, mode).is_some();
    let own_tools = store.dirs().tools_dir();
    let has_tool =
        kotonoha_platform::tools::locate(Tool::Whisper, store.settings().tools.whisper.as_deref(), Some(&own_tools))
            .is_some();
    has_model && has_tool
}
