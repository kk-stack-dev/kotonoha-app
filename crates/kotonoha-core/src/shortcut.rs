//! Global shortcut model and its per-OS presentation.
//!
//! Labels use plain words ("Option + Space", "Ctrl + Alt + Space"), never OS-specific glyphs
//! such as the Mac modifier symbols, so they render identically on every system font.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    MacOS,
    Windows,
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Modifier keys. Ordered the way each OS conventionally lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    /// Command on macOS, the Windows key on Windows.
    Super,
}

impl Modifier {
    pub fn label(self, platform: Platform) -> &'static str {
        match (self, platform) {
            (Self::Ctrl, Platform::MacOS) => "Control",
            (Self::Ctrl, _) => "Ctrl",
            (Self::Alt, Platform::MacOS) => "Option",
            (Self::Alt, _) => "Alt",
            (Self::Shift, _) => "Shift",
            (Self::Super, Platform::MacOS) => "Command",
            (Self::Super, Platform::Windows) => "Win",
            (Self::Super, Platform::Linux) => "Super",
        }
    }

    fn accelerator(self) -> &'static str {
        match self {
            Self::Ctrl => "Control",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Super => "Super",
        }
    }
}

/// A key combination. `key` is a W3C `KeyboardEvent.code` value ("Space", "KeyA", "F2"),
/// which names the physical key independent of keyboard layout and IME state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    pub modifiers: BTreeSet<Modifier>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    #[error("a shortcut needs a key")]
    MissingKey,
    #[error("a modifier key cannot be the main key")]
    ModifierAsKey,
    #[error("letter, digit and space keys need at least one modifier")]
    NeedsModifier,
}

impl Shortcut {
    pub fn new(modifiers: impl IntoIterator<Item = Modifier>, key: impl Into<String>) -> Result<Self, ShortcutError> {
        let shortcut = Self { modifiers: modifiers.into_iter().collect(), key: key.into() };
        shortcut.validate()?;
        Ok(shortcut)
    }

    /// Option+Space on macOS. On Windows Alt+Space opens the window menu, so Ctrl+Alt+Space is used.
    pub fn default_for(platform: Platform) -> Self {
        let modifiers: &[Modifier] = match platform {
            Platform::MacOS => &[Modifier::Alt],
            Platform::Windows | Platform::Linux => &[Modifier::Ctrl, Modifier::Alt],
        };
        Self { modifiers: modifiers.iter().copied().collect(), key: "Space".into() }
    }

    pub fn validate(&self) -> Result<(), ShortcutError> {
        if self.key.is_empty() {
            return Err(ShortcutError::MissingKey);
        }
        if is_modifier_code(&self.key) {
            return Err(ShortcutError::ModifierAsKey);
        }
        let is_function_key = self.key.starts_with('F') && self.key[1..].parse::<u8>().is_ok();
        if self.modifiers.is_empty() && !is_function_key {
            return Err(ShortcutError::NeedsModifier);
        }
        Ok(())
    }

    /// Human-readable label, e.g. "Option + Space" or "Ctrl + Alt + Space".
    pub fn label(&self, platform: Platform) -> String {
        self.modifiers
            .iter()
            .map(|m| m.label(platform).to_owned())
            .chain(std::iter::once(key_label(&self.key)))
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// Accelerator string understood by the global shortcut backend, e.g. "Alt+Space".
    pub fn accelerator(&self) -> String {
        self.modifiers
            .iter()
            .map(|m| m.accelerator())
            .chain(std::iter::once(self.key.as_str()))
            .collect::<Vec<_>>()
            .join("+")
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label(Platform::current()))
    }
}

/// The key combination that pastes on `platform`, for messages like "paste with Ctrl + V".
pub fn paste_label(platform: Platform) -> String {
    let modifier = if platform == Platform::MacOS { Modifier::Super } else { Modifier::Ctrl };
    format!("{} + V", modifier.label(platform))
}

fn is_modifier_code(code: &str) -> bool {
    ["Control", "Alt", "Shift", "Meta", "OS"].iter().any(|m| code.starts_with(m))
}

/// "KeyA" -> "A", "Digit1" -> "1", "ArrowUp" -> "Up", everything else as is.
pub fn key_label(code: &str) -> String {
    if let Some(letter) = code.strip_prefix("Key") {
        return letter.to_owned();
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return digit.to_owned();
    }
    if let Some(dir) = code.strip_prefix("Arrow") {
        return dir.to_owned();
    }
    match code {
        "Escape" => "Esc".into(),
        "Backquote" => "`".into(),
        "Minus" => "-".into(),
        "Equal" => "=".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Slash" => "/".into(),
        "Semicolon" => ";".into(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_use_plain_words_per_platform() {
        let s = Shortcut::new([Modifier::Super, Modifier::Shift], "Space").unwrap();
        assert_eq!(s.label(Platform::MacOS), "Shift + Command + Space");
        assert_eq!(s.label(Platform::Windows), "Shift + Win + Space");
        assert_eq!(Shortcut::default_for(Platform::MacOS).label(Platform::MacOS), "Option + Space");
        assert_eq!(Shortcut::default_for(Platform::Windows).label(Platform::Windows), "Ctrl + Alt + Space");
        assert_eq!(Shortcut::new([], "F2").unwrap().label(Platform::Windows), "F2");
        assert_eq!(Shortcut::new([Modifier::Ctrl], "KeyK").unwrap().label(Platform::MacOS), "Control + K");
    }

    #[test]
    fn accelerator_strings() {
        assert_eq!(Shortcut::default_for(Platform::MacOS).accelerator(), "Alt+Space");
        assert_eq!(Shortcut::default_for(Platform::Windows).accelerator(), "Control+Alt+Space");
    }

    #[test]
    fn validation() {
        assert_eq!(Shortcut::new([], "KeyA"), Err(ShortcutError::NeedsModifier));
        assert_eq!(Shortcut::new([Modifier::Alt], "AltLeft"), Err(ShortcutError::ModifierAsKey));
        assert_eq!(Shortcut::new([Modifier::Alt], ""), Err(ShortcutError::MissingKey));
        assert!(Shortcut::new([], "F12").is_ok());
    }

    #[test]
    fn paste_labels() {
        assert_eq!(paste_label(Platform::MacOS), "Command + V");
        assert_eq!(paste_label(Platform::Windows), "Ctrl + V");
    }
}
