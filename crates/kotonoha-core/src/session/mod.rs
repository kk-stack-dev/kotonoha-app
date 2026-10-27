//! One dictation session: record, transcribe while recording, clean up, deliver.
//!
//! The controller is an actor driven by commands, so a shortcut press never waits on audio
//! hardware, Whisper or the LLM. Presses that arrive while the session is busy are ignored.

mod controller;
mod phase;

pub use controller::{spawn, Services, SessionHandle, Timing};
pub use phase::{Failure, Outcome, Phase};
