use kotonoha_core::lan::protocol::{DecodeError, ErrorKind};

#[derive(Debug, thiserror::Error)]
pub enum LanError {
    /// The TCP connection could not be made.
    #[error("could not connect: {0}")]
    Connect(std::io::Error),
    #[error("connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("encryption failed: {0}")]
    Noise(#[from] snow::Error),
    #[error("timed out")]
    TimedOut,
    #[error("message too large")]
    TooLarge,
    #[error("unexpected message: {0}")]
    Protocol(String),
    /// The computer at that address is not the paired one.
    #[error("a different computer answered")]
    WrongPeer,
    /// The other computer answered with an error.
    #[error("the other computer refused ({kind:?}): {message}")]
    Refused { kind: ErrorKind, message: String },
    /// The person declined the pairing on the other computer (or did not answer in time).
    #[error("pairing was declined")]
    Declined,
    #[error("address is outside this LAN")]
    NotLan,
    #[error("key file: {0}")]
    Key(String),
    #[error("discovery failed: {0}")]
    Discovery(String),
}

impl From<DecodeError> for LanError {
    fn from(e: DecodeError) -> Self {
        Self::Protocol(e.to_string())
    }
}

impl LanError {
    /// macOS refuses LAN connections from an app without the local network permission with "no route
    /// to host" (EHOSTUNREACH), even to a computer that is up.
    pub fn may_be_local_network_permission(&self) -> bool {
        cfg!(target_os = "macos") && matches!(self, Self::Connect(e) if e.raw_os_error() == Some(65))
    }
}
