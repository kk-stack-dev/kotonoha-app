//! Daily log files in the platform log directory. Spoken text is never logged, only sizes and timings.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};

/// Keep the returned guard alive for the lifetime of the app, or buffered lines are lost.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,kotonoha_core=info,kotonoha_platform=info,kotonoha_app=info"));
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("kotonoha")
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir);
    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            fmt().with_env_filter(filter).with_writer(writer).with_ansi(false).init();
            Some(guard)
        }
        Err(e) => {
            fmt().with_env_filter(filter).init();
            tracing::warn!(error = %e, "could not open log directory; logging to stderr");
            None
        }
    }
}
