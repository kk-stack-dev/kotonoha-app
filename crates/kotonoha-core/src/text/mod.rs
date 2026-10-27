//! Pure text rules applied to transcripts. Every function here is deterministic.

pub mod chars;
pub mod correction;
pub mod edit_guard;
pub mod filler;
pub mod integrity;
pub mod lines;
pub mod overlap;
pub mod punctuation;
pub mod spoken;
pub mod transcript;
pub mod vocabulary;
