//! Update notice. A check runs once a while after launch (unless switched off in settings) and, when a
//! newer release exists, adds an item to the tray menu and a banner to the settings window. Nothing is
//! downloaded or installed; the item opens the release page. The settings window can also start the
//! update itself, on a person's confirmation, by running the install script bundled with this build.

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

/// The install scripts of this build. They download the latest release and check it against the
/// release's SHA256SUMS, exactly as when a person runs them from the README.
#[cfg(target_os = "macos")]
const INSTALL_SCRIPT: &str = include_str!("../../../install.sh");
#[cfg(windows)]
const INSTALL_SCRIPT: &str = include_str!("../../../install.ps1");

/// Runs the install script and waits for it. The script quits and replaces this app once the download
/// is verified, so on success this process is gone before the call returns. Returns a message for the
/// person when the script fails early (offline, damaged download), leaving the running app untouched.
#[cfg(any(target_os = "macos", windows))]
pub fn run_installer() -> Result<(), String> {
    use std::process::{Command, Stdio};

    let dir = std::env::temp_dir();
    #[cfg(target_os = "macos")]
    let mut command = {
        let path = dir.join("kotonoha-update.sh");
        std::fs::write(&path, INSTALL_SCRIPT).map_err(|e| e.to_string())?;
        let mut command = Command::new("/bin/sh");
        command.arg(&path);
        command
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        // Windows PowerShell reads a script without a byte order mark as ANSI, which garbles Japanese;
        // it also prints in the OEM code page unless told otherwise
        let path = dir.join("kotonoha-update.ps1");
        let body = format!("\u{feff}[Console]::OutputEncoding = [Text.Encoding]::UTF8\r\n{INSTALL_SCRIPT}");
        std::fs::write(&path, body).map_err(|e| e.to_string())?;
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(&path);
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    };

    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|_| "更新を始められませんでした。".to_string())?;
    if output.status.success() {
        return Ok(());
    }
    tracing::warn!(code = ?output.status.code(), "the update script failed");
    Err(failure_message(&String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr)))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn run_installer() -> Result<(), String> {
    Err("この OS では、アプリからの更新に対応していません。".to_string())
}

/// The script's own failure line ("... できませんでした: reason"), or a general one.
fn failure_message(stdout: &str, stderr: &str) -> String {
    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| line.contains("できませんでした"))
        .map(str::to_owned)
        .unwrap_or_else(|| "更新できませんでした。しばらくしてから、もう一度お試しください。".to_string())
}

#[cfg(test)]
mod tests {
    use super::failure_message;

    #[test]
    fn picks_the_scripts_failure_line() {
        let out = "Kotonoha の新しい版をダウンロードしています\n";
        let err = "\n更新できませんでした: ダウンロードに失敗しました。インターネットの接続を確かめてください\n";
        assert_eq!(
            failure_message(out, err),
            "更新できませんでした: ダウンロードに失敗しました。インターネットの接続を確かめてください"
        );
    }

    #[test]
    fn falls_back_to_a_general_message() {
        assert!(failure_message("", "curl: (6) could not resolve host").starts_with("更新できませんでした。"));
    }
}
