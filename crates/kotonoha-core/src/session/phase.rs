use serde::Serialize;

use crate::ports::CopyReason;

/// What the session is doing, as shown by the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Phase {
    Idle,
    /// Opening the microphone (can take a moment while a Bluetooth headset switches profile).
    Starting,
    Recording {
        elapsed_secs: u32,
        limit_secs: u32,
    },
    Transcribing {
        remaining_chunks: usize,
    },
    Rewriting,
    Inserting,
    Finished {
        outcome: Outcome,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Outcome {
    Inserted,
    Copied { reason: CopyReason },
    Failed { failure: Failure },
}

/// Why a session produced no text. Detail strings are diagnostics, never transcribed speech.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "camelCase")]
pub enum Failure {
    /// No Whisper model file was found.
    ModelMissing,
    MicPermission,
    MicUnavailable(String),
    /// Opening the microphone did not finish in time.
    MicStartTimedOut,
    /// The input device went away during recording and nothing usable was captured.
    MicLost(String),
    /// Recording ran, but the named input device delivered no samples or only digital silence
    /// (muted, a closed laptop lid, or a microphone permission that silences capture).
    NoAudio(String),
    TooShort,
    /// Audio arrived but no speech was recognised.
    NothingRecognized,
    Transcription(String),
}
