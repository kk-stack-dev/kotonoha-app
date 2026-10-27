//! The whisper.cpp and llama.cpp executables: their names, and the builds the app can download.
//!
//! The installer stays small because these are fetched during first-run setup, the same way models are.
//! Builds come from the `tools-*` releases of the public kotonoha-app repository and are pinned by
//! SHA-256 here, so a download is used only when it is byte-for-byte the build that was published.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    Whisper,
    Llama,
}

impl Tool {
    /// Executable names, preferred first. Newer llama.cpp turned `llama-cli` into an interactive chat that
    /// echoes a banner and the prompt, so the one-shot `llama-completion` comes first.
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Self::Whisper => &["whisper-cli", "whisper-cpp"],
            Self::Llama => &["llama-completion", "llama-cli"],
        }
    }

    pub fn display_name(self) -> &'static str {
        self.names()[0]
    }
}

/// A machine the release builds tools for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    MacArm64,
    MacX64,
    WindowsX64,
}

impl Target {
    /// The machine this binary was built for, or `None` where no tool builds are published.
    pub fn current() -> Option<Self> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::MacArm64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Self::MacX64)
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            Some(Self::WindowsX64)
        } else {
            None
        }
    }

    /// File name of `tool` on this target, as it is saved and looked up.
    pub fn executable(self, tool: Tool) -> String {
        match self {
            Self::WindowsX64 => format!("{}.exe", tool.display_name()),
            Self::MacArm64 | Self::MacX64 => tool.display_name().to_owned(),
        }
    }
}

/// A tool build the app can download, with its expected checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTool {
    pub tool: Tool,
    pub target: Target,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

impl CatalogTool {
    pub fn file(&self) -> String {
        self.target.executable(self.tool)
    }
}

/// Filled from the `catalog.rs` attached to a release of https://github.com/kk-stack-dev/kotonoha-app.
/// Until a target has an entry, setup on that machine falls back to telling the user how to install the tool.
/// tools-v1: whisper.cpp v1.9.5, llama.cpp v0.7.0.
pub const TOOL_CATALOG: &[CatalogTool] = &[
    CatalogTool {
        tool: Tool::Whisper,
        target: Target::MacArm64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/whisper-cli-mac-arm64",
        sha256: "4e178127dd8f22be907e0d6315c6bbfc98e482cb88a8dee042a350890197b305",
        bytes: 8005736,
    },
    CatalogTool {
        tool: Tool::Whisper,
        target: Target::MacX64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/whisper-cli-mac-x64",
        sha256: "a72825ae40aae17c02a0ede2279637bf9e6ebad0fce203dd7e60bd6be59b33cb",
        bytes: 3229480,
    },
    CatalogTool {
        tool: Tool::Whisper,
        target: Target::WindowsX64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/whisper-cli-windows-x64.exe",
        sha256: "a16e6b02a777a640aa21786858a4efea480a88ce2bd582030efa3a2744d398ec",
        bytes: 2711552,
    },
    CatalogTool {
        tool: Tool::Llama,
        target: Target::MacArm64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/llama-completion-mac-arm64",
        sha256: "43247fb4fdf8fd24667234a63d6df54d5788774540bd1f57a2d6dc7481435486",
        bytes: 15478496,
    },
    CatalogTool {
        tool: Tool::Llama,
        target: Target::MacX64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/llama-completion-mac-x64",
        sha256: "4f818f0819ce49d851f8786052edfba8620b7a202d8057d5ad717f1d0d0c1b70",
        bytes: 11160944,
    },
    CatalogTool {
        tool: Tool::Llama,
        target: Target::WindowsX64,
        url: "https://github.com/kk-stack-dev/kotonoha-app/releases/download/tools-v1/llama-completion-windows-x64.exe",
        sha256: "3f03aad894fa242bbc25e78bc1ee8907e47f315de8819987850c97a276cca279",
        bytes: 7312896,
    },
];

pub fn downloadable(
    catalog: &'static [CatalogTool],
    tool: Tool,
    target: Option<Target>,
) -> Option<&'static CatalogTool> {
    let target = target?;
    catalog.iter().find(|t| t.tool == tool && t.target == target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &[CatalogTool] = &[CatalogTool {
        tool: Tool::Whisper,
        target: Target::WindowsX64,
        url: "https://example.invalid/whisper-cli.exe",
        sha256: "00",
        bytes: 1,
    }];

    #[test]
    fn looks_up_by_tool_and_target() {
        assert!(downloadable(FAKE, Tool::Whisper, Some(Target::WindowsX64)).is_some());
        assert!(downloadable(FAKE, Tool::Llama, Some(Target::WindowsX64)).is_none());
        assert!(downloadable(FAKE, Tool::Whisper, Some(Target::MacArm64)).is_none());
        assert!(downloadable(FAKE, Tool::Whisper, None).is_none());
        assert_eq!(FAKE[0].file(), "whisper-cli.exe");
        assert_eq!(Target::MacArm64.executable(Tool::Llama), "llama-completion");
    }

    #[test]
    fn published_entries_are_complete() {
        for t in TOOL_CATALOG {
            assert_eq!(t.sha256.len(), 64, "{t:?}");
            assert!(t.bytes > 0 && t.url.starts_with("https://"), "{t:?}");
        }
    }
}
