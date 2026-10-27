//! Microphone permission. On macOS a denied or undecided permission does not make capture fail: the
//! device just delivers silence. So the status is checked explicitly before opening the device.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicPermission {
    Granted,
    Denied,
    /// The user has not been asked yet.
    NotDetermined,
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    use super::MicPermission;

    pub fn microphone_permission() -> MicPermission {
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return MicPermission::Granted;
        };
        // SAFETY: AVMediaTypeAudio is a valid media type for this call
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
            AVAuthorizationStatus::Authorized => MicPermission::Granted,
            AVAuthorizationStatus::NotDetermined => MicPermission::NotDetermined,
            _ => MicPermission::Denied,
        }
    }

    /// Shows the system prompt if the user has not been asked, and waits up to `wait` for the answer.
    pub fn request_microphone_permission(wait: Duration) -> MicPermission {
        let current = microphone_permission();
        if current != MicPermission::NotDetermined {
            return current;
        }
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return current;
        };
        let (tx, rx) = mpsc::channel();
        let handler = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: valid media type; the block is retained by AVFoundation until it runs
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler) };
        match rx.recv_timeout(wait) {
            Ok(true) => MicPermission::Granted,
            Ok(false) => MicPermission::Denied,
            Err(_) => MicPermission::NotDetermined,
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::time::Duration;

    use super::MicPermission;

    /// Windows reports a blocked microphone as an access error when the device is opened.
    pub fn microphone_permission() -> MicPermission {
        MicPermission::Granted
    }

    pub fn request_microphone_permission(_: Duration) -> MicPermission {
        MicPermission::Granted
    }
}

pub use imp::{microphone_permission, request_microphone_permission};
