//! Processing on another computer in the same LAN.
//!
//! A computer that "accepts work" listens on the LAN and runs whisper.cpp / llama.cpp for computers
//! paired with it. Audio and text only ever travel to an address inside the LAN, over an encrypted
//! connection to a computer whose key the user confirmed with a six-digit code. The rules for that
//! (which addresses count as LAN, how the code is made, the message format) live here; sockets,
//! encryption and discovery live in `kotonoha-platform`.

pub mod address;
pub mod pairing;
pub mod protocol;
mod route;

pub(crate) use route::{RemoteLlm, RoutedRecognizer};

/// The DNS-SD service a computer that accepts work announces on the LAN.
pub const SERVICE_TYPE: &str = "_kotonoha._tcp.local.";
/// The TCP port a computer that accepts work listens on.
pub const DEFAULT_PORT: u16 = 50540;
