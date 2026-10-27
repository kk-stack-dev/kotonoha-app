//! Settings and dictionary, kept as JSON files in the config directory.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use kotonoha_core::dictionary::{Dictionary, Entry, EntrySource};
use kotonoha_core::ports::ConfigSource;
use kotonoha_core::settings::Settings;
use serde::{de::DeserializeOwned, Serialize};

use crate::paths::AppDirs;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not write {}: {source}", path.display())]
    Write { path: PathBuf, source: std::io::Error },
    #[error(transparent)]
    Invalid(#[from] serde_json::Error),
}

/// Loads both files once and writes them back on every change.
pub struct ConfigStore {
    dirs: AppDirs,
    settings: RwLock<Settings>,
    dictionary: RwLock<Dictionary>,
}

impl ConfigStore {
    /// A missing or unreadable file starts from defaults (and is logged), so a corrupt file never
    /// keeps the app from starting. The built-in dictionary words are always merged in.
    pub fn open(dirs: AppDirs) -> Self {
        let settings: Settings = read_json(&dirs.settings_file()).unwrap_or_default();
        let mut dictionary: Dictionary = read_json(&dirs.dictionary_file()).unwrap_or_default();
        dictionary.merge(Dictionary::builtin().entries);
        Self { dirs, settings: RwLock::new(settings), dictionary: RwLock::new(dictionary) }
    }

    pub fn dirs(&self) -> &AppDirs {
        &self.dirs
    }

    pub fn models_dir(&self) -> PathBuf {
        self.settings().models.directory.unwrap_or_else(|| self.dirs.default_models_dir())
    }

    pub fn save_settings(&self, settings: Settings) -> Result<(), StoreError> {
        write_json(&self.dirs.settings_file(), &settings)?;
        *self.settings.write().unwrap() = settings;
        Ok(())
    }

    /// Applies `change` to the dictionary and saves it. Returns what `change` returned.
    pub fn update_dictionary<T>(&self, change: impl FnOnce(&mut Dictionary) -> T) -> Result<T, StoreError> {
        let mut dictionary = self.dictionary.write().unwrap();
        let mut next = dictionary.clone();
        let result = change(&mut next);
        write_json(&self.dirs.dictionary_file(), &next)?;
        *dictionary = next;
        Ok(result)
    }

    /// Imports a personal dictionary in JSON (`[{"word": ..., "reading": ...}]`). Returns how many
    /// entries the file had.
    pub fn import_word_list(&self, file: &Path) -> Result<usize, StoreError> {
        let json =
            std::fs::read_to_string(file).map_err(|source| StoreError::Write { path: file.to_owned(), source })?;
        let entries: Vec<Entry> = kotonoha_core::dictionary::parse_word_list_json(&json)?;
        let count = entries.len();
        self.update_dictionary(|d| d.merge(entries.into_iter().map(|e| Entry { source: EntrySource::User, ..e })))?;
        Ok(count)
    }
}

impl ConfigSource for ConfigStore {
    fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    fn dictionary(&self) -> Dictionary {
        self.dictionary.read().unwrap().clone()
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "could not read file; using defaults");
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| tracing::warn!(path = %path.display(), error = %e, "invalid file; using defaults"))
        .ok()
}

/// Writes to a sibling temporary file and renames it over the target, so a crash never leaves half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let err = |source| StoreError::Write { path: path.to_owned(), source };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(err)?;
    serde_json::to_writer_pretty(&mut tmp, value)?;
    tmp.persist(path).map_err(|e| err(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_reloads() {
        let root = tempfile::tempdir().unwrap();
        let store = ConfigStore::open(AppDirs::under(root.path()));
        assert!(store.dictionary().entries.iter().any(|e| e.word == "Claude"));

        let settings = Settings { auto_insert: false, ..store.settings() };
        store.save_settings(settings).unwrap();
        assert!(store.update_dictionary(|d| d.add("言の葉", vec!["ことのは".into()])).unwrap());

        let reopened = ConfigStore::open(AppDirs::under(root.path()));
        assert!(!reopened.settings().auto_insert);
        assert!(reopened.dictionary().entries.iter().any(|e| e.word == "言の葉" && e.source == EntrySource::User));
    }

    #[test]
    fn corrupt_files_fall_back_to_defaults() {
        let root = tempfile::tempdir().unwrap();
        let dirs = AppDirs::under(root.path());
        std::fs::create_dir_all(&dirs.config).unwrap();
        std::fs::write(dirs.settings_file(), "{ not json").unwrap();
        let store = ConfigStore::open(dirs);
        assert_eq!(store.settings(), Settings::default());
    }

    #[test]
    fn imports_a_word_list() {
        let root = tempfile::tempdir().unwrap();
        let store = ConfigStore::open(AppDirs::under(root.path()));
        let file = root.path().join("dictionary.json");
        std::fs::write(&file, r#"[{"word": "ひだまり工房", "reading": "日だまり工房"}]"#).unwrap();
        assert_eq!(store.import_word_list(&file).unwrap(), 1);
        assert!(store.dictionary().entries.iter().any(|e| e.word == "ひだまり工房"));
    }
}
