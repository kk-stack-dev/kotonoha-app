//! Text completion with the llama.cpp command-line tool, run as a separate process per request.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use kotonoha_core::ports::{CompletionRequest, EngineError, LanguageModel};

use crate::process;
use crate::tools::{self, Tool};
use crate::whisper::to_engine_error;

const CONTEXT_TOKENS: usize = 4_096;

pub struct LlamaCli {
    configured: Option<PathBuf>,
    app_dir: Option<PathBuf>,
    threads: usize,
}

impl LlamaCli {
    pub fn new(configured: Option<PathBuf>, app_dir: Option<PathBuf>) -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(1, 8));
        Self { configured, app_dir, threads }
    }

    pub fn executable(&self) -> Option<PathBuf> {
        tools::locate(Tool::Llama, self.configured.as_deref(), self.app_dir.as_deref())
    }
}

#[async_trait]
impl LanguageModel for LlamaCli {
    async fn complete(&self, request: CompletionRequest<'_>) -> Result<String, EngineError> {
        let program = self.executable().ok_or_else(|| EngineError::ToolMissing(Tool::Llama.display_name().into()))?;
        if !request.model.is_file() {
            return Err(EngineError::ModelMissing(request.model.to_owned()));
        }
        // The prompt goes through a UTF-8 file: Japanese text in argv is mangled by the ANSI code page
        // on Windows, and a file has no command-line length limit
        let prompt =
            write_prompt(request.prompt).map_err(|e| EngineError::Failed(format!("could not write prompt: {e}")))?;
        let args: Vec<std::ffi::OsString> = vec![
            "-m".into(),
            request.model.into(),
            "-f".into(),
            prompt.path().into(),
            "-c".into(),
            CONTEXT_TOKENS.to_string().into(),
            "-t".into(),
            self.threads.to_string().into(),
            "-n".into(),
            request.max_tokens.to_string().into(),
            // Greedy decoding: the same speech always gives the same text
            "--temp".into(),
            "0".into(),
            "-no-cnv".into(),
            "--no-display-prompt".into(),
            "--no-warmup".into(),
        ];
        // Roughly: model load plus a token budget proportional to the request
        let timeout = Duration::from_secs(30) + Duration::from_millis(request.max_tokens as u64 * 50);
        let output = process::run(&program, &args, timeout).await.map_err(to_engine_error)?;
        if !output.success {
            return Err(EngineError::Failed(format!(
                "llama exited with {:?}: {}",
                output.code,
                process::tail(&output.stderr, 300)
            )));
        }
        Ok(output.stdout)
    }
}

fn write_prompt(prompt: &str) -> std::io::Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let mut file = tempfile::Builder::new().prefix("kotonoha-prompt-").suffix(".txt").tempfile()?;
    file.write_all(prompt.as_bytes())?;
    file.flush()?;
    Ok(file)
}
