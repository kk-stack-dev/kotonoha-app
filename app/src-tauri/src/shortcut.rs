//! The global recording shortcuts. The tap shortcut starts and stops recording with one press each;
//! the optional hold shortcut records only while it is held down. They are separate keys.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use kotonoha_core::session::SessionHandle;
use kotonoha_core::settings::Settings;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new().build()
}

/// Replaces whatever is registered with the shortcuts in `settings`.
pub fn register<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<(), String> {
    let manager = app.global_shortcut();
    manager.unregister_all().map_err(|e| e.to_string())?;

    manager
        .on_shortcut(settings.shortcut.accelerator().as_str(), |app, _shortcut, event| {
            // Only the press counts; the release of a tap does nothing
            if event.state() == ShortcutState::Pressed {
                if let Some(session) = app.try_state::<SessionHandle>() {
                    session.toggle();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    tracing::info!(shortcut = %settings.shortcut.accelerator(), "tap shortcut registered");

    if let Some(hold) = &settings.hold_shortcut {
        // Some backends repeat "pressed" while a key is held; only the first one of a hold counts
        let down = Arc::new(AtomicBool::new(false));
        let registered = manager.on_shortcut(hold.accelerator().as_str(), move |app, _shortcut, event| {
            let Some(session) = app.try_state::<SessionHandle>() else { return };
            match event.state() {
                ShortcutState::Pressed => {
                    if !down.swap(true, Ordering::SeqCst) {
                        session.hold_start();
                    }
                }
                ShortcutState::Released => {
                    down.store(false, Ordering::SeqCst);
                    session.hold_end();
                }
            }
        });
        if let Err(e) = registered {
            let _ = manager.unregister_all();
            return Err(e.to_string());
        }
        tracing::info!(shortcut = %hold.accelerator(), "hold shortcut registered");
    }
    Ok(())
}

/// Frees the shortcuts while the settings page records a new one, so pressing one does not start a recording.
pub fn suspend<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())
}
