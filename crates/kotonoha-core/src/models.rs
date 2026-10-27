//! Which Whisper / LLM model files to use, chosen from free memory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemoryMode {
    Normal,
    /// Smaller models.
    Low,
    /// Smallest Whisper model and no LLM.
    Critical,
}

impl MemoryMode {
    pub fn from_available_mb(available_mb: u64) -> Self {
        match available_mb {
            0..1_500 => Self::Critical,
            1_500..3_500 => Self::Low,
            _ => Self::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    Whisper,
    Llm,
}

impl ModelKind {
    /// whisper.cpp models are `ggml-*.bin`, llama.cpp models are `*.gguf`.
    pub fn of_file(name: &str) -> Option<Self> {
        if name.starts_with("ggml-") && name.ends_with(".bin") {
            Some(Self::Whisper)
        } else if name.ends_with(".gguf") {
            Some(Self::Llm)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub name: String,
    pub path: PathBuf,
    pub kind: ModelKind,
    pub bytes: u64,
}

/// Preference order by mode. With `kotonoha bench asr` (7 sentences read by 4 macOS voices, built-in
/// dictionary), large-v3-turbo q5_0 had a 0.3% error rate and small 4.9%, at about 0.8 GB peak memory each.
fn whisper_preference(mode: MemoryMode) -> &'static [&'static str] {
    match mode {
        MemoryMode::Normal => &["large-v3-turbo", "medium", "small", "base", "tiny"],
        MemoryMode::Low | MemoryMode::Critical => &["small", "base", "tiny"],
    }
}

/// Preference order by mode. On 18 clean-up cases that share no sentence with the prompt examples (final
/// error rate after the edit guard): Qwen2.5-1.5B 0.8% at 1.3 GB, Qwen3.5-0.8B 0.3% at 0.76 GB,
/// Qwen2.5-0.5B 2.3% at 0.66 GB peak memory.
fn llm_preference(mode: MemoryMode) -> &'static [&'static str] {
    match mode {
        MemoryMode::Normal => &["1.5b", "1.7b", "0.8b", "0.5b", "0.6b"],
        MemoryMode::Low => &["0.8b", "0.5b", "0.6b"],
        MemoryMode::Critical => &[],
    }
}

pub fn choose_whisper(models: &[ModelFile], mode: MemoryMode) -> Option<&ModelFile> {
    choose(models, ModelKind::Whisper, whisper_preference(mode)).or_else(|| first_of(models, ModelKind::Whisper))
}

/// No LLM at all in critical mode; the rule-based text is used instead.
pub fn choose_llm(models: &[ModelFile], mode: MemoryMode) -> Option<&ModelFile> {
    if mode == MemoryMode::Critical {
        return None;
    }
    choose(models, ModelKind::Llm, llm_preference(mode)).or_else(|| first_of(models, ModelKind::Llm))
}

fn choose<'a>(models: &'a [ModelFile], kind: ModelKind, preference: &[&str]) -> Option<&'a ModelFile> {
    preference.iter().find_map(|tag| models.iter().find(|m| m.kind == kind && m.name.to_lowercase().contains(tag)))
}

fn first_of(models: &[ModelFile], kind: ModelKind) -> Option<&ModelFile> {
    models.iter().find(|m| m.kind == kind)
}

/// Which of the two sizes a catalog model is. The UI calls them 標準モデル and 軽量モデル.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelTier {
    Standard,
    /// For machines short on memory: a little less accurate, about half the size.
    Light,
}

/// A model the app can download, with its expected checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub file: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub url: &'static str,
    pub kind: ModelKind,
    pub tier: ModelTier,
}

pub const CATALOG: &[CatalogModel] = &[
    CatalogModel {
        file: "ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        bytes: 574_041_195,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        kind: ModelKind::Whisper,
        tier: ModelTier::Standard,
    },
    CatalogModel {
        file: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        bytes: 1_117_320_736,
        url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
        kind: ModelKind::Llm,
        tier: ModelTier::Standard,
    },
    CatalogModel {
        file: "ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        bytes: 487_601_967,
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        kind: ModelKind::Whisper,
        tier: ModelTier::Light,
    },
    CatalogModel {
        file: "Qwen3.5-0.8B-Q4_K_M.gguf",
        sha256: "bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517",
        bytes: 532_517_120,
        url: "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf",
        kind: ModelKind::Llm,
        tier: ModelTier::Light,
    },
];

/// The catalog model of `kind` that suits `mode`: standard models normally, light ones when memory is
/// short, and no LLM at all in critical mode (matching [`choose_llm`]).
pub fn catalog_for(kind: ModelKind, mode: MemoryMode) -> Option<&'static CatalogModel> {
    let tier = match (kind, mode) {
        (ModelKind::Llm, MemoryMode::Critical) => return None,
        (_, MemoryMode::Normal) => ModelTier::Standard,
        (_, MemoryMode::Low | MemoryMode::Critical) => ModelTier::Light,
    };
    CATALOG.iter().find(|m| m.kind == kind && m.tier == tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> ModelFile {
        ModelFile { name: name.into(), path: name.into(), kind: ModelKind::of_file(name).unwrap(), bytes: 1 }
    }

    #[test]
    fn mode_thresholds() {
        assert_eq!(MemoryMode::from_available_mb(1_000), MemoryMode::Critical);
        assert_eq!(MemoryMode::from_available_mb(2_000), MemoryMode::Low);
        assert_eq!(MemoryMode::from_available_mb(8_000), MemoryMode::Normal);
    }

    #[test]
    fn picks_by_mode() {
        let models: Vec<ModelFile> = CATALOG.iter().map(|c| file(c.file)).collect();
        assert_eq!(choose_whisper(&models, MemoryMode::Normal).unwrap().name, "ggml-large-v3-turbo-q5_0.bin");
        assert_eq!(choose_whisper(&models, MemoryMode::Low).unwrap().name, "ggml-small.bin");
        assert_eq!(choose_llm(&models, MemoryMode::Normal).unwrap().name, "qwen2.5-1.5b-instruct-q4_k_m.gguf");
        assert_eq!(choose_llm(&models, MemoryMode::Low).unwrap().name, "Qwen3.5-0.8B-Q4_K_M.gguf");
        assert!(choose_llm(&models, MemoryMode::Critical).is_none());
    }

    #[test]
    fn catalog_pick_matches_what_automatic_choice_would_use() {
        for mode in [MemoryMode::Normal, MemoryMode::Low, MemoryMode::Critical] {
            let picked: Vec<ModelFile> = [ModelKind::Whisper, ModelKind::Llm]
                .into_iter()
                .filter_map(|kind| catalog_for(kind, mode))
                .map(|m| file(m.file))
                .collect();
            let whisper = catalog_for(ModelKind::Whisper, mode).unwrap();
            assert_eq!(choose_whisper(&picked, mode).unwrap().name, whisper.file);
            assert_eq!(
                choose_llm(&picked, mode).map(|m| m.name.as_str()),
                catalog_for(ModelKind::Llm, mode).map(|m| m.file)
            );
        }
        assert!(catalog_for(ModelKind::Llm, MemoryMode::Critical).is_none());
        assert_eq!(catalog_for(ModelKind::Whisper, MemoryMode::Critical).unwrap().tier, ModelTier::Light);
    }

    #[test]
    fn older_low_memory_model_is_still_used_when_it_is_the_only_one() {
        let models = vec![file("qwen2.5-0.5b-instruct-q4_k_m.gguf")];
        assert_eq!(choose_llm(&models, MemoryMode::Low).unwrap().name, "qwen2.5-0.5b-instruct-q4_k_m.gguf");
    }

    #[test]
    fn falls_back_to_any_model_of_the_kind() {
        let models = vec![file("ggml-large-v3-turbo-q5_0.bin")];
        assert_eq!(choose_whisper(&models, MemoryMode::Critical).unwrap().name, "ggml-large-v3-turbo-q5_0.bin");
        assert!(choose_whisper(&[], MemoryMode::Normal).is_none());
        assert_eq!(ModelKind::of_file("notes.txt"), None);
    }
}
