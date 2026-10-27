//! Runs whisper.cpp / llama.cpp safely: stdin is closed so an interactive tool can never wait for
//! keyboard input, output is drained concurrently so a full pipe cannot deadlock, and the process
//! is killed at the timeout so a stuck tool never stalls a session.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub code: Option<i32>,
    pub elapsed: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("could not start {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} timed out after {timeout:?}")]
    TimedOut { program: String, timeout: Duration },
    #[error("{program} failed while running: {source}")]
    Io { program: String, source: std::io::Error },
}

pub async fn run<I, S>(program: &Path, args: I, timeout: Duration) -> Result<ToolOutput, ToolError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let name = program.file_name().map_or_else(|| program.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    hide_console_window(&mut command);

    let started = Instant::now();
    let child = command.spawn().map_err(|source| ToolError::Spawn { program: name.clone(), source })?;
    // Dropping the future on timeout drops the child, which kills it (kill_on_drop)
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => Err(ToolError::TimedOut { program: name, timeout }),
        Ok(Err(source)) => Err(ToolError::Io { program: name, source }),
        Ok(Ok(out)) => Ok(ToolOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            success: out.status.success(),
            code: out.status.code(),
            elapsed: started.elapsed(),
        }),
    }
}

/// Console programs started from a GUI app on Windows flash a console window unless told not to.
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_: &mut Command) {}

/// The last `max_chars` characters of a tool's stderr, for error messages.
pub fn tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(max_chars)).collect::<String>().trim().to_owned()
}

/// The lines of a tool's stderr that report an error, for error messages. whisper.cpp and llama.cpp
/// print their timings after the error, so the plain tail hides the cause. Falls back to the tail.
pub fn error_summary(stderr: &str, max_chars: usize) -> String {
    let errors = stderr
        .lines()
        .map(str::trim)
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("error") || l.contains("failed")
        })
        .collect::<Vec<_>>()
        .join("\n");
    tail(if errors.is_empty() { stderr } else { &errors }, max_chars)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kills_a_hung_process_at_the_timeout() {
        let started = Instant::now();
        let result = run(Path::new("/bin/sleep"), ["10"], Duration::from_millis(300)).await;
        assert!(matches!(result, Err(ToolError::TimedOut { .. })));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn stdin_is_closed() {
        let out = run(Path::new("/bin/cat"), Vec::<&str>::new(), Duration::from_secs(3)).await.unwrap();
        assert!(out.success);
    }

    #[tokio::test]
    async fn large_stderr_does_not_deadlock() {
        let script = "head -c 300000 /dev/zero | tr '\\0' 'x' 1>&2; echo ok";
        let out = run(Path::new("/bin/sh"), ["-c", script], Duration::from_secs(10)).await.unwrap();
        assert_eq!(out.stdout.trim(), "ok");
        assert_eq!(out.stderr.len(), 300_000);
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
    }

    #[test]
    fn error_summary_keeps_the_error_lines() {
        let stderr = "read_audio_data: failed to read audio data\r\nerror: failed to read audio file 'a.wav'\r\n\
                      whisper_print_timings:    total time =   341.34 ms\r\nerror: 1 input file(s) failed\r\n";
        assert_eq!(
            error_summary(stderr, 300),
            "read_audio_data: failed to read audio data\nerror: failed to read audio file 'a.wav'\nerror: 1 input file(s) failed"
        );
        assert_eq!(error_summary("just timings\n", 300), "just timings");
    }
}
