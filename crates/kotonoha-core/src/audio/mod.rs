//! Recorded audio: 16 kHz mono samples, cut into overlapping chunks for transcription.

mod chunker;
mod level;
mod recording;

pub use chunker::{Chunk, ChunkPlan, Chunker};
pub use level::{is_silent, peak_rms, SILENCE_THRESHOLD};
pub use recording::Recording;

/// Sample rate every adapter must deliver and every transcriber receives.
pub const SAMPLE_RATE: u32 = 16_000;
