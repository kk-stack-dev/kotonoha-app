//! Interfaces the application needs from the outside world. Adapters live in `kotonoha-platform`;
//! tests use in-memory fakes.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::dictionary::Dictionary;
use crate::models::ModelFile;
use crate::settings::Settings;

/// Errors from an external engine (Whisper, the LLM). The message is for logs and the UI,
/// and never contains transcribed text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("executable not found: {0}")]
    ToolMissing(String),
    #[error("model file not found: {}", .0.display())]
    ModelMissing(PathBuf),
    #[error("timed out")]
    TimedOut,
    #[error("{0}")]
    Failed(String),
}

pub struct TranscriptionRequest<'a> {
    /// 16 kHz mono samples.
    pub samples: &'a [f32],
    pub model: &'a Path,
    pub language: &'a str,
    /// Words that bias recognition, as "Claude、Codex。".
    pub prompt: &'a str,
}

#[async_trait]
pub trait SpeechRecognizer: Send + Sync {
    /// Raw recognizer output; cleaning happens in the core.
    async fn transcribe(&self, request: TranscriptionRequest<'_>) -> Result<String, EngineError>;
}

pub struct CompletionRequest<'a> {
    pub model: &'a Path,
    pub prompt: &'a str,
    pub max_tokens: usize,
}

#[async_trait]
pub trait LanguageModel: Send + Sync {
    /// Raw completion text for a fully formatted prompt (deterministic, temperature 0).
    async fn complete(&self, request: CompletionRequest<'_>) -> Result<String, EngineError>;
}

/// What a running microphone stream reports.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    /// 16 kHz mono samples.
    Samples(Vec<f32>),
    /// The input device changed or vanished and capture was rebuilt on it.
    Restarted { device: String },
    /// Capture stopped and could not be rebuilt.
    Lost(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MicError {
    #[error("microphone access is not allowed")]
    PermissionDenied,
    #[error("no input device")]
    NoDevice,
    #[error("{0}")]
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicInfo {
    pub device: String,
    pub sample_rate: u32,
}

pub trait Microphone: Send + Sync {
    /// Starts capture and sends events until the returned stream is stopped.
    /// May block (Bluetooth headsets switch profile on open), so callers run it off the async runtime.
    fn open(&self, events: UnboundedSender<AudioEvent>) -> Result<(Box<dyn MicStream>, MicInfo), MicError>;
}

pub trait MicStream: Send {
    fn stop(self: Box<Self>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryOptions {
    pub auto_insert: bool,
    pub keep_on_clipboard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyReason {
    AutoInsertOff,
    /// macOS Accessibility permission is missing.
    PermissionMissing,
    /// The focused field is a password field.
    SecureField,
    PasteFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "reason", rename_all = "camelCase")]
pub enum Delivery {
    Inserted,
    /// Only copied to the clipboard; the user pastes it.
    Copied(CopyReason),
}

#[async_trait]
pub trait TextSink: Send + Sync {
    /// Puts the text on the clipboard first (so it is never lost), then pastes it if allowed.
    async fn deliver(&self, text: &str, options: DeliveryOptions) -> Delivery;
}

pub trait SystemProbe: Send + Sync {
    fn available_memory_mb(&self) -> u64;
}

pub trait ModelLibrary: Send + Sync {
    fn models(&self) -> Vec<ModelFile>;
}

/// Current settings and dictionary, read at the start of each session.
pub trait ConfigSource: Send + Sync {
    fn settings(&self) -> Settings;
    fn dictionary(&self) -> Dictionary;
}
