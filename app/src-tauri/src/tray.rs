//! Menu bar (macOS) / notification area (Windows) icon and menu.

use kotonoha_core::session::{Phase, SessionHandle};
use tauri::image::Image;
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::windows;

const TRAY_ID: &str = "main";
const TOGGLE: &str = "toggle";
const SETTINGS: &str = "settings";
const LOGS: &str = "logs";
const UPDATE: &str = "update";
const QUIT: &str = "quit";

const IDLE_ICON: &[u8] = include_bytes!("../icons/tray/idle.png");
const RECORDING_ICON: &[u8] = include_bytes!("../icons/tray/recording.png");
const BUSY_ICON: &[u8] = include_bytes!("../icons/tray/busy.png");

/// Kept in app state so the toggle item's label can follow the session.
pub struct TrayMenu<R: Runtime> {
    toggle: MenuItem<R>,
    menu: Menu<R>,
    update: Mutex<Option<MenuItem<R>>>,
}

pub fn create<R: Runtime>(app: &AppHandle<R>, tooltip: &str) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, TOGGLE, "録音を開始", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, SETTINGS, "設定を開く", true, None::<&str>)?,
            &MenuItem::with_id(app, LOGS, "ログフォルダを開く", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Kotonoha Voice を終了", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(IDLE_ICON)?)
        .icon_as_template(true)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TOGGLE => {
                if let Some(session) = app.try_state::<SessionHandle>() {
                    session.toggle();
                }
            }
            SETTINGS => windows::open_settings(app),
            LOGS => crate::commands::open_logs_folder(app),
            UPDATE => crate::commands::open_release_page(app.clone()),
            QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(TrayMenu { toggle, menu, update: Mutex::new(None) });
    Ok(())
}

/// Adds (or relabels) the menu item that tells a newer version exists and opens its download page.
pub fn show_update<R: Runtime>(app: &AppHandle<R>, latest: &str) {
    let Some(tray_menu) = app.try_state::<TrayMenu<R>>() else { return };
    let label = format!("最新バージョン {latest} が利用可能です");
    let Ok(mut slot) = tray_menu.update.lock() else { return };
    if let Some(item) = slot.as_ref() {
        let _ = item.set_text(label);
        return;
    }
    let Ok(item) = MenuItem::with_id(app, UPDATE, label, true, None::<&str>) else { return };
    let Ok(separator) = PredefinedMenuItem::separator(app) else { return };
    if tray_menu.menu.insert(&item, 0).is_ok() {
        let _ = tray_menu.menu.insert(&separator, 1);
        *slot = Some(item);
    }
}

pub fn set_tooltip<R: Runtime>(app: &AppHandle<R>, tooltip: &str) {
    if let Some(tray) = tray(app) {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

pub fn show_phase<R: Runtime>(app: &AppHandle<R>, phase: &Phase) {
    let (icon, label, enabled) = match phase {
        Phase::Recording { .. } => (RECORDING_ICON, "録音を停止", true),
        Phase::Starting | Phase::Transcribing { .. } | Phase::Rewriting | Phase::Inserting => {
            (BUSY_ICON, "処理中", false)
        }
        Phase::Idle | Phase::Finished { .. } => (IDLE_ICON, "録音を開始", true),
    };
    if let Some(tray) = tray(app) {
        if let Ok(image) = Image::from_bytes(icon) {
            let _ = tray.set_icon(Some(image));
            let _ = tray.set_icon_as_template(true);
        }
    }
    if let Some(menu) = app.try_state::<TrayMenu<R>>() {
        let _ = menu.toggle.set_text(label);
        let _ = menu.toggle.set_enabled(enabled);
    }
}

fn tray<R: Runtime>(app: &AppHandle<R>) -> Option<TrayIcon<R>> {
    app.tray_by_id(TRAY_ID)
}
