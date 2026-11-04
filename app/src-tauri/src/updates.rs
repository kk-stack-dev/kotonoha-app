//! Update notice. A check runs once a while after launch (unless switched off in settings) and, when a
//! newer release exists, adds an item to the tray menu and a banner to the settings window. Nothing is
//! downloaded or installed; the item opens the release page.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kotonoha_core::ports::ConfigSource;
use kotonoha_core::update::UpdateStatus;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::update;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The newer version found by the latest check, if any.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<String>>);

impl PendingUpdate {
    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|v| v.clone())
    }
}

/// Remembers a newer version and tells the tray and the settings window.
pub fn announce<R: Runtime>(app: &AppHandle<R>, latest: &str) {
    if let Some(pending) = app.try_state::<PendingUpdate>() {
        if let Ok(mut slot) = pending.0.lock() {
            *slot = Some(latest.to_string());
        }
    }
    crate::tray::show_update(app, latest);
    let _ = app.emit("update-available", latest);
}

/// Checks in the background shortly after launch. Failures (offline, rate limit) are silent.
pub fn spawn_launch_check<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || {
        // Let startup (windows, permission prompt) settle first
        std::thread::sleep(Duration::from_secs(8));
        let enabled = app.try_state::<Arc<ConfigStore>>().is_some_and(|s| s.settings().check_updates_on_launch);
        if !enabled {
            return;
        }
        match update::check(env!("CARGO_PKG_VERSION")) {
            Ok(UpdateStatus::Available { latest }) => {
                tracing::info!("a newer version is available");
                announce(&app, &latest);
            }
            Ok(UpdateStatus::UpToDate) => tracing::info!("up to date"),
            Err(e) => tracing::info!(error = %e, "launch update check failed"),
        }
    });
}
