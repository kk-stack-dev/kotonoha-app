//! Finds the whisper.cpp and llama.cpp executables.

use std::path::{Path, PathBuf};

use kotonoha_core::tools::CatalogTool;
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

/// Whether this machine has the Vulkan runtime the Vulkan builds link against. GPU drivers from NVIDIA,
/// AMD and Intel install it; without it a Vulkan build would not even start.
pub fn vulkan_available() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join("System32").join("vulkan-1.dll").is_file()
}

/// Whether the tool found at `found` can stay as it is. Only builds that setup downloaded into the app's
/// own folder (`own_dir`) are managed: one there that is not byte-for-byte `wanted`, the build setup
/// would download now, is out of date. Tools the user installed elsewhere are always kept.
pub fn is_current(found: &Path, own_dir: &Path, wanted: Option<&CatalogTool>) -> bool {
    let Some(wanted) = wanted else { return true };
    if found.parent() != Some(own_dir) {
        return true;
    }
    std::fs::metadata(found).is_ok_and(|m| m.len() == wanted.bytes)
        && crate::download::sha256_file(found).is_ok_and(|sum| sum == wanted.sha256)
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
    fn only_own_downloads_are_replaced() {
        use kotonoha_core::tools::{Target, Variant};

        let own = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let wanted = CatalogTool {
            tool: Tool::Whisper,
            target: Target::WindowsX64,
            variant: Variant::Vulkan,
            url: "https://example.invalid/whisper-cli.exe",
            // SHA-256 of "abc"
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            bytes: 3,
        };
        let mine = own.path().join("whisper-cli.exe");
        std::fs::write(&mine, "abc").unwrap();
        assert!(is_current(&mine, own.path(), Some(&wanted)));
        // An older build in the app's folder is replaced
        std::fs::write(&mine, "abd").unwrap();
        assert!(!is_current(&mine, own.path(), Some(&wanted)));
        assert!(is_current(&mine, own.path(), None));
        // The user's own install is left alone
        let theirs = elsewhere.path().join("whisper-cli.exe");
        std::fs::write(&theirs, "xyz").unwrap();
        assert!(is_current(&theirs, own.path(), Some(&wanted)));
    }

    #[test]
    fn app_dir_is_searched() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join(executable("llama-completion"));
        std::fs::write(&tool, "").unwrap();
        assert_eq!(locate(Tool::Llama, None, Some(dir.path())), Some(tool));
    }
}
