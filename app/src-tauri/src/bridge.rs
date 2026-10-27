//! Forwards session state to the UI: events for the web views, the overlay's visibility and the tray icon.

use std::time::Duration;

use kotonoha_core::session::{Phase, SessionHandle};
use tauri::{AppHandle, Emitter, Runtime};

use crate::{tray, windows};

pub const PHASE_EVENT: &str = "session-phase";
pub const LEVEL_EVENT: &str = "session-level";
const LEVEL_INTERVAL: Duration = Duration::from_millis(80);

pub fn spawn<R: Runtime>(app: AppHandle<R>, session: SessionHandle) {
    let mut phase = session.phase();
    let phase_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let current = phase.borrow_and_update().clone();
            show(&phase_app, &current);
            if phase.changed().await.is_err() {
                return;
            }
        }
    });

    // The level changes with every audio buffer; the overlay only needs a dozen updates a second
    let level = session.level();
    let recording = session.phase();
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(LEVEL_INTERVAL);
        loop {
            ticker.tick().await;
            if matches!(*recording.borrow(), Phase::Recording { .. }) {
                let _ = app.emit(LEVEL_EVENT, *level.borrow());
            }
        }
    });
}

fn show<R: Runtime>(app: &AppHandle<R>, phase: &Phase) {
    let _ = app.emit(PHASE_EVENT, phase);
    tray::show_phase(app, phase);
    match phase {
        Phase::Idle => windows::hide_overlay(app),
        _ => windows::show_overlay(app),
    }
}
