use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::time::{self, Instant, MissedTickBehavior};

use super::phase::{Failure, Outcome, Phase};
use crate::audio::{self, Chunk, ChunkPlan, Chunker, Recording};
use crate::dictionary::DEFAULT_PROMPT_BUDGET;
use crate::models::{self, MemoryMode};
use crate::pipeline::{self, LlmPlan, PipelineOptions, SkipReason};
use crate::ports::{
    AudioEvent, ConfigSource, Delivery, DeliveryOptions, LanguageModel, MicError, MicStream, Microphone, ModelLibrary,
    SpeechRecognizer, SystemProbe, TextSink, TranscriptionRequest,
};
use crate::text::transcript;

/// Everything the session needs from the outside world.
#[derive(Clone)]
pub struct Services {
    pub microphone: Arc<dyn Microphone>,
    pub recognizer: Arc<dyn SpeechRecognizer>,
    pub llm: Arc<dyn LanguageModel>,
    pub sink: Arc<dyn TextSink>,
    pub probe: Arc<dyn SystemProbe>,
    pub models: Arc<dyn ModelLibrary>,
    pub config: Arc<dyn ConfigSource>,
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Opening the microphone is abandoned after this long.
    pub mic_start_timeout: Duration,
    pub mic_stop_timeout: Duration,
    /// How long a finished result stays visible before returning to idle.
    pub result_linger: Duration,
    /// A recording with no samples at all for this long is reported as "no audio", not "too short".
    pub min_silent_recording: Duration,
    /// A shortcut held at least this long records only while held; a shorter press toggles.
    pub hold_to_talk: Duration,
    pub chunks: ChunkPlan,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            mic_start_timeout: Duration::from_secs(6),
            mic_stop_timeout: Duration::from_secs(3),
            result_linger: Duration::from_millis(1_800),
            min_silent_recording: Duration::from_secs(1),
            hold_to_talk: Duration::from_millis(400),
            chunks: ChunkPlan::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Toggle,
    /// The shortcut went down: toggles, and remembers when, so that a long hold can stop on release.
    Press,
    Release,
    Cancel,
}

/// Cheap, cloneable handle used by the shortcut, the tray menu and the UI.
#[derive(Clone)]
pub struct SessionHandle {
    commands: mpsc::Sender<Command>,
    phase: watch::Receiver<Phase>,
    level: watch::Receiver<f32>,
}

impl SessionHandle {
    /// Starts or stops recording. Never blocks; returns false if the controller is gone or flooded.
    pub fn toggle(&self) -> bool {
        self.commands.try_send(Command::Toggle).is_ok()
    }

    /// The shortcut went down. A tap toggles recording; holding it records until [`Self::release`].
    pub fn press(&self) -> bool {
        self.commands.try_send(Command::Press).is_ok()
    }

    pub fn release(&self) -> bool {
        self.commands.try_send(Command::Release).is_ok()
    }

    /// Stops recording and discards it.
    pub fn cancel(&self) -> bool {
        self.commands.try_send(Command::Cancel).is_ok()
    }

    pub fn phase(&self) -> watch::Receiver<Phase> {
        self.phase.clone()
    }

    /// Input level (peak RMS of the latest audio) while recording, 0 otherwise.
    pub fn level(&self) -> watch::Receiver<f32> {
        self.level.clone()
    }
}

/// Starts the session controller on the current Tokio runtime.
pub fn spawn(services: Services, timing: Timing) -> SessionHandle {
    let (commands_tx, commands) = mpsc::channel(8);
    let (phase_tx, phase) = watch::channel(Phase::Idle);
    let (level_tx, level) = watch::channel(0.0);
    let controller = Controller { services, timing, commands, phase: phase_tx, level: level_tx };
    tokio::spawn(controller.run());
    SessionHandle { commands: commands_tx, phase, level }
}

struct Controller {
    services: Services,
    timing: Timing,
    commands: mpsc::Receiver<Command>,
    phase: watch::Sender<Phase>,
    level: watch::Sender<f32>,
}

#[derive(Debug, PartialEq)]
enum StopReason {
    User,
    Limit,
    Cancelled,
    Lost(String),
}

struct ChunkResult {
    index: usize,
    text: String,
    error: Option<String>,
}

impl Controller {
    async fn run(mut self) {
        let mut next = self.commands.recv().await;
        while let Some(command) = next {
            next = match command {
                Command::Cancel | Command::Release => self.commands.recv().await,
                Command::Toggle | Command::Press => match self.session(command).await {
                    None => {
                        self.phase.send_replace(Phase::Idle);
                        self.commands.recv().await
                    }
                    Some(outcome) => self.linger(outcome).await,
                },
            };
        }
    }

    /// Shows the outcome until it expires or the next command arrives (which is then handled).
    async fn linger(&mut self, outcome: Outcome) -> Option<Command> {
        self.phase.send_replace(Phase::Finished { outcome });
        let next = tokio::select! {
            () = time::sleep(self.timing.result_linger) => None,
            command = self.commands.recv() => Some(command),
        };
        self.phase.send_replace(Phase::Idle);
        match next {
            None => self.commands.recv().await,
            Some(command) => command,
        }
    }

    /// Runs one session, started by `trigger`. `None` means it was cancelled.
    async fn session(&mut self, trigger: Command) -> Option<Outcome> {
        let mut held_since = (trigger == Command::Press).then(Instant::now);
        self.phase.send_replace(Phase::Starting);
        let settings = self.services.config.settings();
        let dictionary = self.services.config.dictionary();
        let mode = MemoryMode::from_available_mb(self.services.probe.available_memory_mb());
        let library = self.services.models.models();

        let Some(whisper_model) =
            settings.models.whisper.clone().or_else(|| models::choose_whisper(&library, mode).map(|m| m.path.clone()))
        else {
            return Some(failed(Failure::ModelMissing));
        };
        tracing::info!(?mode, model = %file_name(&whisper_model), "session starting");

        let (audio_tx, mut audio) = mpsc::unbounded_channel();
        let (stream, mut device, released) = match self.open_microphone(audio_tx, &mut held_since).await {
            Ok(opened) => opened,
            Err(failure) => return Some(failed(failure)),
        };

        // Recording, with chunks transcribed in the background as they fill up
        let (chunk_tx, chunk_rx) = mpsc::unbounded_channel();
        let (result_tx, mut result_rx) = mpsc::unbounded_channel();
        let worker = tokio::spawn(transcribe_chunks(
            self.services.recognizer.clone(),
            chunk_rx,
            result_tx,
            whisper_model,
            settings.language.clone(),
            dictionary.whisper_prompt(DEFAULT_PROMPT_BUDGET),
        ));

        let limit = Duration::from_secs(u64::from(settings.max_recording_seconds));
        let mut recording = Recording::with_max_seconds(limit.as_secs_f64() + 5.0);
        let mut chunker = Chunker::new(self.timing.chunks);
        let mut results = Vec::new();
        let mut sent = 0;
        let started = Instant::now();
        let limit_secs = settings.max_recording_seconds;
        self.phase.send_replace(Phase::Recording { elapsed_secs: 0, limit_secs });

        let mut ticker = time::interval(Duration::from_millis(250));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut early_stop = released;
        let stop = loop {
            if let Some(stop) = early_stop.take() {
                break stop;
            }
            tokio::select! {
                event = audio.recv() => match event {
                    Some(AudioEvent::Samples(samples)) => {
                        recording.append(&samples);
                        self.level.send_replace(audio::peak_rms(&samples));
                        for chunk in chunker.poll(recording.samples()) {
                            sent += 1;
                            let _ = chunk_tx.send(chunk);
                        }
                    }
                    Some(AudioEvent::Restarted { device: restarted }) => {
                        tracing::info!(device = %restarted, "audio input restarted");
                        device = restarted;
                    }
                    Some(AudioEvent::Lost(reason)) => break StopReason::Lost(reason),
                    None => break StopReason::Lost("audio stream ended".into()),
                },
                command = self.commands.recv() => match command {
                    Some(Command::Toggle | Command::Press) => break StopReason::User,
                    Some(Command::Release) => early_stop = self.hold_ended(&mut held_since),
                    Some(Command::Cancel) | None => break StopReason::Cancelled,
                },
                Some(result) = result_rx.recv() => results.push(result),
                _ = ticker.tick() => {
                    let elapsed = started.elapsed();
                    if elapsed >= limit {
                        break StopReason::Limit;
                    }
                    let elapsed_secs = elapsed.as_secs() as u32;
                    self.phase.send_if_modified(|p| match p {
                        Phase::Recording { elapsed_secs: shown, .. } if *shown != elapsed_secs => {
                            *shown = elapsed_secs;
                            true
                        }
                        _ => false,
                    });
                }
            }
        };
        let recorded = started.elapsed();
        self.close_microphone(stream).await;
        while let Ok(event) = audio.try_recv() {
            if let AudioEvent::Samples(samples) = event {
                recording.append(&samples);
            }
        }
        self.level.send_replace(0.0);
        tracing::info!(
            ?stop,
            recorded_secs = recorded.as_secs_f64(),
            captured_secs = recording.seconds(),
            "recording stopped"
        );

        if stop == StopReason::Cancelled {
            worker.abort();
            return None;
        }

        // Audio that was still queued when recording stopped gets the same chunking as the rest
        for chunk in chunker.poll(recording.samples()) {
            sent += 1;
            let _ = chunk_tx.send(chunk);
        }
        if let Some(last) = chunker.finish(recording.samples()) {
            sent += 1;
            let _ = chunk_tx.send(last);
        }
        drop(chunk_tx);
        while results.len() < sent {
            self.phase.send_replace(Phase::Transcribing { remaining_chunks: sent - results.len() });
            match ignore_commands(&mut self.commands, result_rx.recv()).await {
                Some(result) => results.push(result),
                None => break,
            }
        }
        results.sort_by_key(|r| r.index);

        let texts: Vec<String> = results.iter().map(|r| r.text.clone()).collect();
        if texts.iter().all(|t| t.trim().is_empty()) {
            let failure = if let Some(error) = results.iter().find_map(|r| r.error.clone()) {
                Failure::Transcription(error)
            } else if let StopReason::Lost(reason) = stop {
                Failure::MicLost(reason)
            } else if recording.is_empty() && recorded >= self.timing.min_silent_recording {
                Failure::NoAudio(device)
            } else if sent == 0 {
                Failure::TooShort
            } else if audio::peak_rms(recording.samples()) == 0.0 {
                // Exact zeros are what a muted or misrouted device delivers, never a quiet room
                Failure::NoAudio(device)
            } else {
                Failure::NothingRecognized
            };
            return Some(failed(failure));
        }

        self.phase.send_replace(Phase::Rewriting);
        let llm_model = if !settings.llm_rewrite {
            Err(SkipReason::Disabled)
        } else if mode == MemoryMode::Critical {
            Err(SkipReason::LowMemory)
        } else {
            settings
                .models
                .llm
                .clone()
                .or_else(|| models::choose_llm(&library, mode).map(|m| m.path.clone()))
                .ok_or(SkipReason::NoModel)
        };
        let plan = match &llm_model {
            Ok(model) => LlmPlan::Use { llm: self.services.llm.as_ref(), model },
            Err(reason) => LlmPlan::Skip(*reason),
        };
        let options = PipelineOptions {
            dictionary: &dictionary,
            remove_fillers: settings.remove_fillers,
            line_breaks: settings.line_breaks,
        };
        let processed = ignore_commands(&mut self.commands, pipeline::process(&texts, options, plan)).await;
        tracing::info!(
            chunks = results.len(),
            transcript_chars = processed.transcript.chars().count(),
            final_chars = processed.final_text.chars().count(),
            llm = ?processed.llm,
            "text processed"
        );
        if processed.final_text.is_empty() {
            return Some(failed(Failure::NothingRecognized));
        }

        self.phase.send_replace(Phase::Inserting);
        let options =
            DeliveryOptions { auto_insert: settings.auto_insert, keep_on_clipboard: settings.keep_on_clipboard };
        let delivery =
            ignore_commands(&mut self.commands, self.services.sink.deliver(&processed.final_text, options)).await;
        tracing::info!(?delivery, "delivered");
        Some(match delivery {
            Delivery::Inserted => Outcome::Inserted,
            Delivery::Copied(reason) => Outcome::Copied { reason },
        })
    }

    /// A release stops the recording only if the press that started it was held long enough;
    /// otherwise that press was a tap, and the recording goes on until the next one.
    fn hold_ended(&self, held_since: &mut Option<Instant>) -> Option<StopReason> {
        let since = held_since.take()?;
        (since.elapsed() >= self.timing.hold_to_talk).then_some(StopReason::User)
    }

    /// Opens the microphone off the async runtime, giving up after the start timeout. A device
    /// that opens late is dropped (and so stopped) by the abandoned blocking task. Presses meanwhile
    /// are ignored (users press again when a start feels slow), but the end of a hold is kept and
    /// returned, so a short hold-to-talk still stops once the device is open.
    async fn open_microphone(
        &mut self,
        events: mpsc::UnboundedSender<AudioEvent>,
        held_since: &mut Option<Instant>,
    ) -> Result<(Box<dyn MicStream>, String, Option<StopReason>), Failure> {
        let microphone = self.services.microphone.clone();
        let open =
            time::timeout(self.timing.mic_start_timeout, tokio::task::spawn_blocking(move || microphone.open(events)));
        tokio::pin!(open);
        let mut released = None;
        let opened = loop {
            tokio::select! {
                opened = &mut open => break opened,
                Some(command) = self.commands.recv() => match command {
                    Command::Release => released = released.or(self.hold_ended(held_since)),
                    _ => tracing::info!(?command, "ignored: microphone is starting"),
                },
            }
        };
        match opened {
            Err(_) => {
                tracing::error!(timeout = ?self.timing.mic_start_timeout, "microphone did not start in time");
                Err(Failure::MicStartTimedOut)
            }
            Ok(Err(join_error)) => Err(Failure::MicUnavailable(join_error.to_string())),
            Ok(Ok(Err(MicError::PermissionDenied))) => Err(Failure::MicPermission),
            Ok(Ok(Err(e))) => Err(Failure::MicUnavailable(e.to_string())),
            Ok(Ok(Ok((stream, info)))) => {
                tracing::info!(device = %info.device, rate = info.sample_rate, "microphone open");
                Ok((stream, info.device, released))
            }
        }
    }

    async fn close_microphone(&self, stream: Box<dyn MicStream>) {
        let stop = tokio::task::spawn_blocking(move || stream.stop());
        if time::timeout(self.timing.mic_stop_timeout, stop).await.is_err() {
            tracing::warn!("microphone did not stop in time; continuing");
        }
    }
}

/// Awaits `work` while dropping (and logging) any commands that arrive meanwhile.
async fn ignore_commands<T>(commands: &mut mpsc::Receiver<Command>, work: impl Future<Output = T>) -> T {
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return output,
            Some(command) = commands.recv() => tracing::info!(?command, "ignored: session is busy"),
        }
    }
}

/// Transcribes chunks one at a time, in order. Silent chunks are skipped, because Whisper invents
/// stock phrases on silence. A failed chunk keeps its slot so ordering is preserved.
async fn transcribe_chunks(
    recognizer: Arc<dyn SpeechRecognizer>,
    mut chunks: mpsc::UnboundedReceiver<Chunk>,
    results: mpsc::UnboundedSender<ChunkResult>,
    model: PathBuf,
    language: String,
    prompt: String,
) {
    while let Some(chunk) = chunks.recv().await {
        let level = audio::peak_rms(&chunk.samples);
        let result = if level < audio::SILENCE_THRESHOLD {
            tracing::info!(index = chunk.index, level, "chunk skipped as silence");
            ChunkResult { index: chunk.index, text: String::new(), error: None }
        } else {
            let started = Instant::now();
            let request =
                TranscriptionRequest { samples: &chunk.samples, model: &model, language: &language, prompt: &prompt };
            match recognizer.transcribe(request).await {
                Ok(raw) => {
                    let text = transcript::clean_whisper_output(&raw);
                    let text = if transcript::is_known_hallucination(&text) { String::new() } else { text };
                    tracing::info!(
                        index = chunk.index,
                        audio_secs = chunk.seconds(),
                        level,
                        took_secs = started.elapsed().as_secs_f64(),
                        chars = text.chars().count(),
                        "chunk transcribed"
                    );
                    ChunkResult { index: chunk.index, text, error: None }
                }
                Err(e) => {
                    tracing::error!(index = chunk.index, error = %e, "chunk transcription failed");
                    ChunkResult { index: chunk.index, text: String::new(), error: Some(e.to_string()) }
                }
            }
        };
        if results.send(result).is_err() {
            return;
        }
    }
}

fn failed(failure: Failure) -> Outcome {
    tracing::warn!(?failure, "session failed");
    Outcome::Failed { failure }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
