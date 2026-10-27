//! Model files found on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kotonoha_core::models::{ModelFile, ModelKind};
use kotonoha_core::ports::ModelLibrary;

/// Scans the models directory each time it is asked, so files added while the app runs are picked up.
pub struct ModelDirectory {
    dir: Arc<dyn Fn() -> PathBuf + Send + Sync>,
}

impl ModelDirectory {
    /// `dir` is re-evaluated on each scan, because the user can change it in settings.
    pub fn new(dir: impl Fn() -> PathBuf + Send + Sync + 'static) -> Self {
        Self { dir: Arc::new(dir) }
    }
}

impl ModelLibrary for ModelDirectory {
    fn models(&self) -> Vec<ModelFile> {
        scan(&(self.dir)())
    }
}

pub fn scan(dir: &Path) -> Vec<ModelFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut models: Vec<ModelFile> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = ModelKind::of_file(&name)?;
            let meta = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some(ModelFile { name, path: entry.path(), kind, bytes: meta.len() })
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_whisper_and_llm_files_only() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["ggml-small.bin", "qwen2.5-0.5b.gguf", "notes.txt", "other.bin"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let found = scan(dir.path());
        let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["ggml-small.bin", "qwen2.5-0.5b.gguf"]);
        assert!(scan(&dir.path().join("missing")).is_empty());
    }
}
