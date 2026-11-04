//! User settings. Every field has a default so older settings files keep loading.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::shortcut::{Platform, Shortcut, ShortcutError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Press once to start recording and again to stop.
    pub shortcut: Shortcut,
    /// Record only while this key is held. `None` leaves hold-to-talk off.
    pub hold_shortcut: Option<Shortcut>,
    /// Recording stops by itself after this long.
    pub max_recording_seconds: u32,
    /// Whisper language code.
    pub language: String,
    pub remove_fillers: bool,
    /// Start a new line after each sentence, or per topic in long text.
    pub line_breaks: bool,
    /// Clean up punctuation and leftover fillers with the local LLM.
    pub llm_rewrite: bool,
    /// Paste the result into the focused input. When off, the text is only copied.
    pub auto_insert: bool,
    /// Leave the result on the clipboard after pasting. When off, the previous clipboard is restored.
    pub keep_on_clipboard: bool,
    /// Look for a newer release a few seconds after launch and show a notice. Nothing is installed.
    pub check_updates_on_launch: bool,
    pub models: ModelSettings,
    pub tools: ToolPaths,
}

impl Settings {
    pub fn default_for(platform: Platform) -> Self {
        Self {
            shortcut: Shortcut::default_for(platform),
            hold_shortcut: None,
            max_recording_seconds: 600,
            language: "ja".into(),
            remove_fillers: true,
            line_breaks: true,
            llm_rewrite: true,
            auto_insert: true,
            keep_on_clipboard: true,
            check_updates_on_launch: true,
            models: ModelSettings::default(),
            tools: ToolPaths::default(),
        }
    }
}

impl Settings {
    /// Both shortcuts must be valid, and the two must differ.
    pub fn validate_shortcuts(&self) -> Result<(), ShortcutError> {
        self.shortcut.validate()?;
        if let Some(hold) = &self.hold_shortcut {
            hold.validate()?;
            if *hold == self.shortcut {
                return Err(ShortcutError::Duplicate);
            }
        }
        Ok(())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::default_for(Platform::current())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelSettings {
    /// Where model files live. `None` uses the app's data directory.
    pub directory: Option<PathBuf>,
    /// Fixed model files. `None` picks one automatically from free memory.
    pub whisper: Option<PathBuf>,
    pub llm: Option<PathBuf>,
}

/// Explicit locations of the whisper.cpp and llama.cpp executables. `None` searches the usual places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolPaths {
    pub whisper: Option<PathBuf>,
    pub llama: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"autoInsert": false}"#).unwrap();
        assert!(!s.auto_insert);
        assert!(s.llm_rewrite);
        assert!(s.check_updates_on_launch);
        assert_eq!(s.max_recording_seconds, 600);
    }

    #[test]
    fn hold_shortcut_is_off_by_default_and_must_differ() {
        let mut s = Settings::default_for(Platform::MacOS);
        assert_eq!(s.hold_shortcut, None);
        assert!(s.validate_shortcuts().is_ok());
        s.hold_shortcut = Some(s.shortcut.clone());
        assert_eq!(s.validate_shortcuts(), Err(ShortcutError::Duplicate));
        s.hold_shortcut = Some(Shortcut::new([], "F5").unwrap());
        assert!(s.validate_shortcuts().is_ok());
        s.hold_shortcut = Some(Shortcut { modifiers: Default::default(), key: "KeyA".into() });
        assert_eq!(s.validate_shortcuts(), Err(ShortcutError::NeedsModifier));
    }

    #[test]
    fn round_trip() {
        let s = Settings::default_for(Platform::Windows);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
