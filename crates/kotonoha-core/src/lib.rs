//! Kotonoha's domain logic and application services.
//!
//! Nothing in this crate touches the OS: audio devices, subprocesses, the clipboard and storage are
//! reached through the traits in [`ports`]. That keeps every rule here testable with plain data.

pub mod audio;
pub mod dictionary;
pub mod linebreak;
pub mod models;
pub mod pipeline;
pub mod ports;
pub mod rewrite;
pub mod session;
pub mod settings;
pub mod setup;
pub mod shortcut;
pub mod text;
pub mod tools;
