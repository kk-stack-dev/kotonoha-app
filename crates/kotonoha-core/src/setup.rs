//! First-run setup: what has to be downloaded before dictation works, and how far along it is.

use serde::Serialize;

use crate::models::{self, CatalogModel, MemoryMode, ModelKind};
use crate::tools::{self, CatalogTool, Target, Tool};

/// Where a downloaded file goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Destination {
    Tools,
    Models,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub file: String,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub destination: Destination,
    /// Shown in the setup list: 文字起こし or 文章の整え.
    pub purpose: ModelKind,
}

impl Download {
    pub fn model(m: &CatalogModel) -> Self {
        Self {
            file: m.file.to_owned(),
            url: m.url,
            sha256: m.sha256,
            bytes: m.bytes,
            destination: Destination::Models,
            purpose: m.kind,
        }
    }

    fn tool(t: &CatalogTool) -> Self {
        let purpose = match t.tool {
            Tool::Whisper => ModelKind::Whisper,
            Tool::Llama => ModelKind::Llm,
        };
        Self { file: t.file(), url: t.url, sha256: t.sha256, bytes: t.bytes, destination: Destination::Tools, purpose }
    }
}

/// What is already on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Present {
    pub whisper_tool: bool,
    pub llama_tool: bool,
    pub whisper_model: bool,
    pub llm_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub downloads: Vec<Download>,
    /// Missing tools with no published build for this machine; the user has to install these.
    pub manual: Vec<Tool>,
}

impl Plan {
    pub fn total_bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.bytes).sum()
    }
}

/// Everything missing for dictation, sized for `mode`. Text clean-up (llama.cpp and its model) is
/// left out in critical mode, where it is never used.
pub fn plan(mode: MemoryMode, target: Option<Target>, present: Present, catalog: &'static [CatalogTool]) -> Plan {
    let mut plan = Plan { downloads: Vec::new(), manual: Vec::new() };
    let wants_llm = mode != MemoryMode::Critical;
    for (tool, have, wanted) in
        [(Tool::Whisper, present.whisper_tool, true), (Tool::Llama, present.llama_tool, wants_llm)]
    {
        if have || !wanted {
            continue;
        }
        match tools::downloadable(catalog, tool, target) {
            Some(build) => plan.downloads.push(Download::tool(build)),
            None => plan.manual.push(tool),
        }
    }
    for (kind, have) in [(ModelKind::Whisper, present.whisper_model), (ModelKind::Llm, present.llm_model)] {
        if !have {
            plan.downloads.extend(models::catalog_for(kind, mode).map(Download::model));
        }
    }
    plan
}

/// Progress over a whole list of downloads, for one bar and one percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// 1-based position of the file being downloaded.
    pub index: usize,
    pub count: usize,
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// `finished` is the bytes of the files already done, `current` the bytes of the current file so far.
    pub fn new(index: usize, count: usize, finished: u64, current: u64, total: u64) -> Self {
        Self { index: index + 1, count, done: (finished + current).min(total), total }
    }

    pub fn percent(&self) -> u64 {
        self.done * 100 / self.total.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILDS: &[CatalogTool] = &[
        CatalogTool {
            tool: Tool::Whisper,
            target: Target::MacArm64,
            url: "https://example.invalid/whisper-cli",
            sha256: "a",
            bytes: 10,
        },
        CatalogTool {
            tool: Tool::Llama,
            target: Target::MacArm64,
            url: "https://example.invalid/llama-completion",
            sha256: "b",
            bytes: 20,
        },
    ];

    fn files(plan: &Plan) -> Vec<&str> {
        plan.downloads.iter().map(|d| d.file.as_str()).collect()
    }

    #[test]
    fn fresh_machine_gets_tools_and_standard_models() {
        let plan = plan(MemoryMode::Normal, Some(Target::MacArm64), Present::default(), BUILDS);
        assert_eq!(
            files(&plan),
            ["whisper-cli", "llama-completion", "ggml-large-v3-turbo-q5_0.bin", "qwen2.5-1.5b-instruct-q4_k_m.gguf"]
        );
        assert!(plan.manual.is_empty());
        assert_eq!(plan.total_bytes(), 30 + 574_041_195 + 1_117_320_736);
        assert_eq!(plan.downloads[0].destination, Destination::Tools);
        assert_eq!(plan.downloads[2].destination, Destination::Models);
    }

    #[test]
    fn low_memory_gets_light_models_and_critical_skips_clean_up() {
        let low = plan(MemoryMode::Low, Some(Target::MacArm64), Present::default(), BUILDS);
        assert_eq!(&files(&low)[2..], ["ggml-small.bin", "Qwen3.5-0.8B-Q4_K_M.gguf"]);
        let critical = plan(MemoryMode::Critical, Some(Target::MacArm64), Present::default(), BUILDS);
        assert_eq!(files(&critical), ["whisper-cli", "ggml-small.bin"]);
    }

    #[test]
    fn only_missing_pieces_are_planned() {
        let present = Present { whisper_tool: true, llama_tool: true, whisper_model: true, llm_model: false };
        let plan = plan(MemoryMode::Normal, Some(Target::MacArm64), present, BUILDS);
        assert_eq!(files(&plan), ["qwen2.5-1.5b-instruct-q4_k_m.gguf"]);
        let all = Present { llm_model: true, ..present };
        assert!(super::plan(MemoryMode::Normal, Some(Target::MacArm64), all, BUILDS).downloads.is_empty());
    }

    #[test]
    fn tools_without_a_build_are_left_to_the_user() {
        let plan = plan(MemoryMode::Normal, Some(Target::WindowsX64), Present::default(), BUILDS);
        assert_eq!(plan.manual, [Tool::Whisper, Tool::Llama]);
        assert_eq!(files(&plan), ["ggml-large-v3-turbo-q5_0.bin", "qwen2.5-1.5b-instruct-q4_k_m.gguf"]);
        assert_eq!(super::plan(MemoryMode::Normal, None, Present::default(), BUILDS).manual.len(), 2);
    }

    #[test]
    fn progress_spans_all_files() {
        let p = Progress::new(1, 2, 100, 50, 300);
        assert_eq!((p.index, p.count, p.done, p.percent()), (2, 2, 150, 50));
        assert_eq!(Progress::new(1, 2, 300, 10, 300).percent(), 100);
        assert_eq!(Progress::new(0, 1, 0, 0, 0).percent(), 0);
    }
}
