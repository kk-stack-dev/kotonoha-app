//! The messages two Kotonoha computers exchange inside the encrypted channel.
//!
//! One connection carries one exchange: a pairing, or a single piece of work. Every message is a JSON
//! header followed by an optional binary body (16-bit audio), framed as
//! `[header length: u32 BE][header JSON][body]`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Raised when the message format changes incompatibly.
pub const VERSION: u32 = 1;
/// Larger messages are refused before they are read. 60 s of 16 kHz 16-bit audio is under 2 MB.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

/// Sent by the computer that asks for work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Request {
    /// Starts a pairing. `commitment` is hex; see [`super::pairing`].
    PairCommit {
        version: u32,
        name: String,
        commitment: String,
    },
    /// Reveals the nonce committed to earlier (hex).
    PairReveal {
        nonce: String,
    },
    /// What this computer can do right now.
    Info {
        version: u32,
    },
    /// The body is 16 kHz mono 16-bit little-endian PCM.
    Transcribe {
        version: u32,
        language: String,
        prompt: String,
    },
    Complete {
        version: u32,
        prompt: String,
        max_tokens: usize,
    },
}

/// Sent by the computer that accepts work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Response {
    PairNonce {
        name: String,
        nonce: String,
    },
    /// The person answered on the accepting computer.
    PairResult {
        accepted: bool,
    },
    Info {
        /// Speech recognition works there.
        ready: bool,
        /// File name of the language model there, which decides the prompt format; `None` when it has none.
        llm_model: Option<String>,
    },
    Text {
        text: String,
    },
    Error {
        kind: ErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The asking computer is not (or no longer) paired.
    NotPaired,
    /// The two computers run incompatible versions.
    Version,
    /// No model or tool for the request.
    Unavailable,
    TimedOut,
    Failed,
    /// Malformed request.
    BadRequest,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("message too short")]
    Truncated,
    #[error("bad message header: {0}")]
    Header(#[from] serde_json::Error),
}

pub fn encode(header: &impl Serialize, body: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(header).expect("message headers serialize");
    let mut out = Vec::with_capacity(4 + json.len() + body.len());
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(body);
    out
}

pub fn decode<T: DeserializeOwned>(message: &[u8]) -> Result<(T, &[u8]), DecodeError> {
    let len = message.get(..4).ok_or(DecodeError::Truncated)?;
    let len = u32::from_be_bytes(len.try_into().expect("4 bytes")) as usize;
    let json = message.get(4..4 + len).ok_or(DecodeError::Truncated)?;
    Ok((serde_json::from_slice(json)?, &message[4 + len..]))
}

/// A computer name received from the other side, made safe to show: no control characters, trimmed,
/// at most 40 characters.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    cleaned.trim().to_owned()
}

/// 16-bit little-endian PCM, half the size of the float samples and the precision whisper.cpp reads.
pub fn samples_to_pcm16(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes()).collect()
}

pub fn pcm16_to_samples(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2).map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / f32::from(i16::MAX)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_body_round_trip() {
        let request = Request::Transcribe { version: VERSION, language: "ja".into(), prompt: "Claude、".into() };
        let bytes = encode(&request, &[1, 2, 3]);
        let (back, body) = decode::<Request>(&bytes).unwrap();
        assert_eq!(back, request);
        assert_eq!(body, [1, 2, 3]);
    }

    #[test]
    fn wire_names_are_stable() {
        let json = serde_json::to_string(&Response::Info { ready: true, llm_model: None }).unwrap();
        assert_eq!(json, r#"{"type":"info","ready":true,"llmModel":null}"#);
        let json =
            serde_json::to_string(&Response::Error { kind: ErrorKind::NotPaired, message: String::new() }).unwrap();
        assert_eq!(json, r#"{"type":"error","kind":"notPaired","message":""}"#);
    }

    #[test]
    fn truncated_messages_are_rejected() {
        assert!(matches!(decode::<Request>(&[0, 0]), Err(DecodeError::Truncated)));
        assert!(matches!(decode::<Request>(&[0, 0, 0, 9, b'{']), Err(DecodeError::Truncated)));
        assert!(matches!(decode::<Request>(&[0, 0, 0, 1, b'{']), Err(DecodeError::Header(_))));
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  書斎の Mac\n"), "書斎の Mac");
        assert_eq!(clean_name("a\u{0}b\u{1b}[31m"), "ab[31m");
        assert_eq!(clean_name(&"あ".repeat(100)).chars().count(), 40);
    }

    #[test]
    fn pcm_round_trip_is_close() {
        let samples = [0.0, 0.5, -0.5, 1.0, -1.0, 2.0];
        let back = pcm16_to_samples(&samples_to_pcm16(&samples));
        assert_eq!(back.len(), samples.len());
        for (a, b) in samples.iter().map(|s| s.clamp(-1.0, 1.0)).zip(back) {
            assert!((a - b).abs() < 1e-4, "{a} {b}");
        }
    }
}
