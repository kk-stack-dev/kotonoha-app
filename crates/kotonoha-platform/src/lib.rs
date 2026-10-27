//! OS adapters for Kotonoha. Each module implements a port from `kotonoha_core::ports`.

pub mod audio;
pub mod download;
pub mod library;
pub mod llama;
pub mod output;
pub mod paths;
pub mod process;
pub mod store;
pub mod system;
pub mod tools;
pub mod whisper;
