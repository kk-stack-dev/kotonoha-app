//! Per-user directories, following each OS's conventions
//! (macOS: ~/Library/Application Support/..., Windows: %APPDATA%\...).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub logs: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Option<Self> {
        let dirs = directories::ProjectDirs::from("dev", "Kotonoha", "Kotonoha")?;
        let data = dirs.data_local_dir().to_owned();
        // macOS keeps logs in ~/Library/Logs, where Console.app finds them
        let logs = if cfg!(target_os = "macos") {
            directories::BaseDirs::new()?.home_dir().join("Library/Logs/Kotonoha")
        } else {
            data.join("logs")
        };
        Some(Self { config: dirs.config_dir().to_owned(), logs, data })
    }

    /// For tests and portable setups.
    pub fn under(root: &Path) -> Self {
        Self { config: root.join("config"), data: root.join("data"), logs: root.join("data").join("logs") }
    }

    pub fn default_models_dir(&self) -> PathBuf {
        self.data.join("models")
    }

    /// whisper.cpp and llama.cpp builds downloaded during setup.
    pub fn tools_dir(&self) -> PathBuf {
        self.data.join("bin")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn dictionary_file(&self) -> PathBuf {
        self.config.join("dictionary.json")
    }
}
