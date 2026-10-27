//! Delivering text to the focused input: clipboard first (so the text is never lost), then a paste
//! keystroke, unless pasting is off, not permitted, or the focused field is a password field.

mod clipboard;
mod focus;
mod keyboard;
mod permission;

use std::time::Duration;

use async_trait::async_trait;
use kotonoha_core::ports::{CopyReason, Delivery, DeliveryOptions, TextSink};

pub use permission::{has_input_permission, open_input_permission_settings, request_input_permission};

/// Time the target app gets to read the clipboard before the previous contents are put back.
const RESTORE_DELAY: Duration = Duration::from_millis(800);
const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Default)]
pub struct PasteSink;

#[async_trait]
impl TextSink for PasteSink {
    async fn deliver(&self, text: &str, options: DeliveryOptions) -> Delivery {
        let previous = if options.keep_on_clipboard { None } else { clipboard::get_text() };
        if let Err(e) = clipboard::set_text(text) {
            // Without the text on the clipboard a paste would insert stale contents
            tracing::error!(error = %e, "could not write the clipboard");
            return Delivery::Copied(CopyReason::PasteFailed);
        }
        if !options.auto_insert {
            return Delivery::Copied(CopyReason::AutoInsertOff);
        }
        if !has_input_permission() {
            tracing::warn!("input permission missing; text left on the clipboard");
            request_input_permission();
            return Delivery::Copied(CopyReason::PermissionMissing);
        }
        if run_blocking(focus::focused_field_is_secure).await.unwrap_or(false) {
            tracing::warn!("focused field is a password field; not pasting");
            return Delivery::Copied(CopyReason::SecureField);
        }

        // The shortcut's modifiers may still be held; Option+Command+V would mean something else
        keyboard::wait_for_modifiers_released(MODIFIER_RELEASE_TIMEOUT).await;
        if let Err(e) = run_blocking(keyboard::send_paste).await.unwrap_or_else(|e| Err(e.to_string())) {
            tracing::error!(error = %e, "paste keystroke failed");
            return Delivery::Copied(CopyReason::PasteFailed);
        }
        if let Some(previous) = previous {
            tokio::time::sleep(RESTORE_DELAY).await;
            if let Err(e) = clipboard::set_text(&previous) {
                tracing::warn!(error = %e, "could not restore the previous clipboard");
            }
        }
        Delivery::Inserted
    }
}

async fn run_blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, tokio::task::JoinError> {
    tokio::task::spawn_blocking(f).await
}
