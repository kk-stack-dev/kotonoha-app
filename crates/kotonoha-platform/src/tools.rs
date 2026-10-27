//! Finds the whisper.cpp and llama.cpp executables.

use std::path::{Path, PathBuf};

pub use kotonoha_core::tools::Tool;

/// Looks for `tool` in, in order: the configured path, the app's own `bin` folder (where setup
/// downloads it), the PATH, and the usual package manager locations (GUI apps on macOS do not inherit
/// the shell PATH).
pub fn locate(tool: Tool, configured: Option<&Path>, app_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured {
        return path.is_file().then(|| path.to_owned());
    }
    let names = tool.names();
    let in_dir = |dir: &Path| names.iter().map(|n| dir.join(executable(n))).find(|p| p.is_file());
    app_dir
        .and_then(in_dir)
        .or_else(|| names.iter().find_map(|n| which::which(n).ok()))
        .or_else(|| well_known_dirs().iter().find_map(|d| in_dir(d)))
}

fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

fn well_known_dirs() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()]
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(|d| vec![PathBuf::from(d).join("Microsoft").join("WinGet").join("Links")])
            .unwrap_or_default()
    } else {
        vec!["/usr/local/bin".into(), "/usr/bin".into()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_path_wins_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("my-whisper");
        std::fs::write(&tool, "").unwrap();
        assert_eq!(locate(Tool::Whisper, Some(&tool), None), Some(tool));
        assert_eq!(locate(Tool::Whisper, Some(&dir.path().join("missing")), None), None);
    }

    #[test]
    fn app_dir_is_searched() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join(executable("llama-completion"));
        std::fs::write(&tool, "").unwrap();
        assert_eq!(locate(Tool::Llama, None, Some(dir.path())), Some(tool));
    }
}
