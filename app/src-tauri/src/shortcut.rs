//! The global recording shortcut. A tap starts and stops recording; holding it records until release.
//! The session decides which from the time between press and release.

use std::sync::atomic::{AtomicBool, Ordering};

use kotonoha_core::session::SessionHandle;
use kotonoha_core::shortcut::Shortcut;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    // Some backends repeat "pressed" while a key is held; only the first one of a hold counts
    let down = AtomicBool::new(false);
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, _shortcut, event| {
            let Some(session) = app.try_state::<SessionHandle>() else { return };
            match event.state() {
                ShortcutState::Pressed => {
                    if !down.swap(true, Ordering::SeqCst) {
                        session.press();
                    }
                }
                ShortcutState::Released => {
                    down.store(false, Ordering::SeqCst);
                    session.release();
                }
            }
        })
        .build()
}

/// Replaces whatever is registered with `shortcut`.
pub fn register<R: Runtime>(app: &AppHandle<R>, shortcut: &Shortcut) -> Result<(), String> {
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;
    manager.register(shortcut.accelerator().as_str()).map_err(|e| e.to_string())?;
    tracing::info!(shortcut = %shortcut.accelerator(), "global shortcut registered");
    Ok(())
}

/// Frees the shortcut while the settings page records a new one, so pressing it does not start a recording.
pub fn suspend<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())
}
