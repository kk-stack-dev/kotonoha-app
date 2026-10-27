//! The recording overlay, the first-run setup window and the settings window.

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const OVERLAY: &str = "overlay";
pub const SETTINGS: &str = "settings";
pub const SETUP: &str = "setup";

const OVERLAY_WIDTH: f64 = 440.0;
const OVERLAY_HEIGHT: f64 = 72.0;
/// Distance from the bottom of the usable screen area, in logical pixels.
const OVERLAY_MARGIN: f64 = 72.0;

/// A small always-on-top panel that can never take focus, so the paste lands in the app the user was typing in.
pub fn create_overlay<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let window = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("overlay.html".into()))
        .title("Kotonoha")
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    Ok(window)
}

pub fn show_overlay<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(OVERLAY) else { return };
    position_overlay(app, &window);
    if let Err(e) = window.show() {
        tracing::warn!(error = %e, "could not show overlay");
    }
}

pub fn hide_overlay<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(OVERLAY) {
        let _ = window.hide();
    }
}

/// Bottom centre of the screen the mouse pointer is on.
fn position_overlay<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let width = (OVERLAY_WIDTH * scale) as i32;
    let height = (OVERLAY_HEIGHT * scale) as i32;
    let x = area.position.x + (area.size.width as i32 - width) / 2;
    let y = area.position.y + area.size.height as i32 - height - (OVERLAY_MARGIN * scale) as i32;
    let _ = window.set_size(LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT));
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

pub fn open_settings<R: Runtime>(app: &AppHandle<R>) {
    open_page(app, SETTINGS, "settings.html", "Kotonoha 設定");
}

pub fn open_setup<R: Runtime>(app: &AppHandle<R>) {
    open_page(app, SETUP, "setup.html", "Kotonoha セットアップ");
}

pub fn close_setup<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(SETUP) {
        let _ = window.close();
    }
}

/// Brings the window forward if it is open, otherwise opens it.
fn open_page<R: Runtime>(app: &AppHandle<R>, label: &str, page: &str, title: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title(title)
        .inner_size(720.0, 760.0)
        .min_inner_size(560.0, 560.0)
        .center()
        .focused(true)
        .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => tracing::error!(error = %e, page, "could not open window"),
    }
}
