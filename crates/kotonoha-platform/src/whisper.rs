//! Speech recognition with the whisper.cpp command-line tool. Running it as a separate process
//! means its ~1 GB of model memory is returned to the OS as soon as a chunk is done.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use kotonoha_core::audio::SAMPLE_RATE;
use kotonoha_core::ports::{EngineError, SpeechRecognizer, TranscriptionRequest};

use crate::process::{self, ToolError};
use crate::tools::{self, Tool};

const TIMEOUT: Duration = Duration::from_secs(120);

pub struct WhisperCli {
    configured: Option<PathBuf>,
    app_dir: Option<PathBuf>,
    threads: usize,
}

impl WhisperCli {
    pub fn new(configured: Option<PathBuf>, app_dir: Option<PathBuf>) -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(1, 8));
        Self { configured, app_dir, threads }
    }

    pub fn executable(&self) -> Option<PathBuf> {
        tools::locate(Tool::Whisper, self.configured.as_deref(), self.app_dir.as_deref())
    }
}

#[async_trait]
impl SpeechRecognizer for WhisperCli {
    async fn transcribe(&self, request: TranscriptionRequest<'_>) -> Result<String, EngineError> {
        let program = self.executable().ok_or_else(|| EngineError::ToolMissing(Tool::Whisper.display_name().into()))?;
        if !request.model.is_file() {
            return Err(EngineError::ModelMissing(request.model.to_owned()));
        }
        // The temporary file is deleted when `wav` drops, after the tool has exited
        let wav = write_wav(request.samples).map_err(|e| EngineError::Failed(format!("could not write audio: {e}")))?;
        let mut args: Vec<std::ffi::OsString> = vec![
            "-m".into(),
            request.model.into(),
            "-f".into(),
            wav.path().into(),
            "-l".into(),
            request.language.into(),
            "-t".into(),
            self.threads.to_string().into(),
            "-nt".into(),
        ];
        if !request.prompt.trim().is_empty() {
            args.extend(["--prompt".into(), request.prompt.into()]);
        }
        let output = process::run(&program, &args, TIMEOUT).await.map_err(to_engine_error)?;
        if !output.success {
            return Err(EngineError::Failed(format!(
                "whisper exited with {:?}: {}",
                output.code,
                process::tail(&output.stderr, 300)
            )));
        }
        Ok(output.stdout)
    }
}

pub(crate) fn to_engine_error(e: ToolError) -> EngineError {
    match e {
        ToolError::TimedOut { .. } => EngineError::TimedOut,
        other => EngineError::Failed(other.to_string()),
    }
}

/// 16-bit PCM WAV, the format whisper.cpp reads natively.
fn write_wav(samples: &[f32]) -> std::io::Result<tempfile::NamedTempFile> {
    let file = tempfile::Builder::new().prefix("kotonoha-").suffix(".wav").tempfile()?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::new(std::io::BufWriter::new(file.as_file()), spec).map_err(std::io::Error::other)?;
    for &s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).map_err(std::io::Error::other)?;
    }
    writer.finalize().map_err(std::io::Error::other)?;
    Ok(file)
}

/// Whether `path` looks like a whisper.cpp model, for settings validation.
pub fn is_model_file(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("ggml-") && n.ends_with(".bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_readable_16k_wav() {
        let file = write_wav(&[0.0, 0.5, -0.5, 1.5]).unwrap();
        let mut reader = hound::WavReader::open(file.path()).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        let samples: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(samples, vec![0, 16_383, -16_383, i16::MAX]);
    }

    #[tokio::test]
    async fn missing_tool_and_model_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let whisper = WhisperCli::new(Some(dir.path().join("none")), None);
        let request = TranscriptionRequest { samples: &[], model: Path::new("/nope.bin"), language: "ja", prompt: "" };
        assert!(matches!(whisper.transcribe(request).await, Err(EngineError::ToolMissing(_))));
    }
}
