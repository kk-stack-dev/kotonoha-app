//! Session behaviour with in-memory fakes. Each test pins a failure seen in the field.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};

use async_trait::async_trait;

use super::*;
use crate::dictionary::Dictionary;
use crate::models::{ModelFile, ModelKind};
use crate::ports::{CompletionRequest, CopyReason, EngineError, MicInfo};
use crate::settings::Settings;

/// What the fake microphone does when opened.
#[derive(Clone)]
enum MicScript {
    /// Sends these sample batches right away, then nothing.
    Samples(Vec<Vec<f32>>),
    /// Blocks until the test releases it (a Bluetooth headset stuck switching profile).
    Hang(Arc<Mutex<Option<std_mpsc::Receiver<()>>>>),
    Fail(MicError),
    /// Sends samples, then reports the device as lost.
    SamplesThenLost(Vec<f32>),
}

struct FakeMic {
    script: MicScript,
    opened: AtomicUsize,
    stopped: Arc<AtomicUsize>,
}

struct FakeStream {
    stopped: Arc<AtomicUsize>,
    _events: mpsc::UnboundedSender<AudioEvent>,
}

impl MicStream for FakeStream {
    fn stop(self: Box<Self>) {
        self.stopped.fetch_add(1, Ordering::SeqCst);
    }
}

impl Microphone for FakeMic {
    fn open(&self, events: mpsc::UnboundedSender<AudioEvent>) -> Result<(Box<dyn MicStream>, MicInfo), MicError> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        match &self.script {
            MicScript::Samples(batches) => {
                for b in batches {
                    events.send(AudioEvent::Samples(b.clone())).unwrap();
                }
            }
            MicScript::Hang(release) => {
                if let Some(rx) = release.lock().unwrap().take() {
                    let _ = rx.recv();
                }
            }
            MicScript::Fail(e) => return Err(e.clone()),
            MicScript::SamplesThenLost(samples) => {
                events.send(AudioEvent::Samples(samples.clone())).unwrap();
                events.send(AudioEvent::Lost("unplugged".into())).unwrap();
            }
        }
        let stream = FakeStream { stopped: self.stopped.clone(), _events: events };
        Ok((Box::new(stream), MicInfo { device: "fake".into(), sample_rate: 16_000 }))
    }
}

/// Returns scripted text per call; `Err` entries fail that call.
struct FakeRecognizer {
    replies: Mutex<Vec<Result<String, EngineError>>>,
    prompts: Mutex<Vec<String>>,
}

#[async_trait]
impl SpeechRecognizer for FakeRecognizer {
    async fn transcribe(&self, request: TranscriptionRequest<'_>) -> Result<String, EngineError> {
        self.prompts.lock().unwrap().push(request.prompt.to_owned());
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            Ok(String::new())
        } else {
            replies.remove(0)
        }
    }
}

struct EchoLlm;

#[async_trait]
impl LanguageModel for EchoLlm {
    async fn complete(&self, request: CompletionRequest<'_>) -> Result<String, EngineError> {
        // The text to clean is the last user turn of the ChatML prompt
        let user = request.prompt.rsplit("<|im_start|>user\n").next().unwrap();
        Ok(user.split("<|im_end|>").next().unwrap().to_owned())
    }
}

#[derive(Default)]
struct FakeSink {
    delivered: Mutex<Vec<String>>,
    copy_only: Option<CopyReason>,
}

#[async_trait]
impl TextSink for FakeSink {
    async fn deliver(&self, text: &str, _: DeliveryOptions) -> Delivery {
        self.delivered.lock().unwrap().push(text.to_owned());
        self.copy_only.map_or(Delivery::Inserted, Delivery::Copied)
    }
}

struct Fixed {
    memory_mb: u64,
    models: Vec<ModelFile>,
    settings: Settings,
}

impl SystemProbe for Fixed {
    fn available_memory_mb(&self) -> u64 {
        self.memory_mb
    }
}

impl ModelLibrary for Fixed {
    fn models(&self) -> Vec<ModelFile> {
        self.models.clone()
    }
}

impl ConfigSource for Fixed {
    fn settings(&self) -> Settings {
        self.settings.clone()
    }
    fn dictionary(&self) -> Dictionary {
        Dictionary::builtin()
    }
}

struct Harness {
    handle: SessionHandle,
    mic: Arc<FakeMic>,
    recognizer: Arc<FakeRecognizer>,
    sink: Arc<FakeSink>,
}

fn whisper_file() -> ModelFile {
    ModelFile {
        name: "ggml-small.bin".into(),
        path: PathBuf::from("/m/ggml-small.bin"),
        kind: ModelKind::Whisper,
        bytes: 1,
    }
}

fn test_timing() -> Timing {
    Timing {
        chunks: ChunkPlan { chunk_seconds: 2.0, overlap_seconds: 0.5, min_final_seconds: 0.3 },
        ..Timing::default()
    }
}

fn harness(
    script: MicScript,
    replies: Vec<Result<String, EngineError>>,
    models: Vec<ModelFile>,
    sink: FakeSink,
) -> Harness {
    harness_with(script, replies, models, sink, test_timing())
}

fn harness_with(
    script: MicScript,
    replies: Vec<Result<String, EngineError>>,
    models: Vec<ModelFile>,
    sink: FakeSink,
    timing: Timing,
) -> Harness {
    let mic = Arc::new(FakeMic { script, opened: AtomicUsize::new(0), stopped: Arc::default() });
    let recognizer = Arc::new(FakeRecognizer { replies: Mutex::new(replies), prompts: Mutex::default() });
    let sink = Arc::new(sink);
    let fixed = Arc::new(Fixed { memory_mb: 8_000, models, settings: Settings::default() });
    let services = Services {
        microphone: mic.clone(),
        recognizer: recognizer.clone(),
        llm: Arc::new(EchoLlm),
        sink: sink.clone(),
        probe: fixed.clone(),
        models: fixed.clone(),
        config: fixed,
    };
    Harness { handle: spawn(services, timing), mic, recognizer, sink }
}

fn tone(seconds: f64) -> Vec<f32> {
    (0..(seconds * 16_000.0) as usize).map(|i| (i as f32 * 0.05).sin() * 0.2).collect()
}

async fn wait_for(handle: &SessionHandle, mut done: impl FnMut(&Phase) -> bool) -> Phase {
    let mut rx = handle.phase();
    let phase =
        time::timeout(Duration::from_secs(60), rx.wait_for(|p| done(p))).await.expect("phase not reached").unwrap();
    phase.clone()
}

async fn finished(handle: &SessionHandle) -> Outcome {
    match wait_for(handle, |p| matches!(p, Phase::Finished { .. })).await {
        Phase::Finished { outcome } => outcome,
        _ => unreachable!(),
    }
}

async fn record_then_stop(h: &Harness) -> Outcome {
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    time::sleep(Duration::from_millis(1_500)).await;
    assert!(h.handle.toggle());
    finished(&h.handle).await
}

#[tokio::test(start_paused = true)]
async fn records_transcribes_cleans_and_inserts() {
    let h = harness(
        MicScript::Samples(vec![tone(1.0), tone(0.5)]),
        vec![Ok("えーっと、クロードで、あー、やります".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert_eq!(record_then_stop(&h).await, Outcome::Inserted);
    assert_eq!(h.sink.delivered.lock().unwrap().as_slice(), ["Claudeで、やります。"]);
    assert!(h.recognizer.prompts.lock().unwrap()[0].contains("Claude"));
    assert_eq!(h.mic.stopped.load(Ordering::SeqCst), 1);
    wait_for(&h.handle, |p| *p == Phase::Idle).await;
}

// Real clock: Tokio does not auto-advance a paused clock while a blocking task is running.
#[tokio::test]
async fn hung_microphone_times_out_and_the_next_press_works() {
    let (release_tx, release_rx) = std_mpsc::channel();
    let timing = Timing { mic_start_timeout: Duration::from_millis(300), ..test_timing() };
    let h = harness_with(
        MicScript::Hang(Arc::new(Mutex::new(Some(release_rx)))),
        vec![],
        vec![whisper_file()],
        FakeSink::default(),
        timing,
    );
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| *p == Phase::Starting).await;
    // Presses while starting are ignored instead of queueing up a stop
    assert!(h.handle.toggle());
    let outcome = finished(&h.handle).await;
    assert_eq!(outcome, Outcome::Failed { failure: Failure::MicStartTimedOut });

    release_tx.send(()).unwrap();
    // The controller is still responsive: the next press starts a new attempt
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    assert_eq!(h.mic.opened.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn no_samples_is_no_audio_not_too_short() {
    let h = harness(MicScript::Samples(vec![]), vec![], vec![whisper_file()], FakeSink::default());
    assert_eq!(record_then_stop(&h).await, Outcome::Failed { failure: Failure::NoAudio("fake".into()) });
}

#[tokio::test(start_paused = true)]
async fn digital_silence_is_no_audio() {
    let h = harness(MicScript::Samples(vec![vec![0.0; 40_000]]), vec![], vec![whisper_file()], FakeSink::default());
    assert_eq!(record_then_stop(&h).await, Outcome::Failed { failure: Failure::NoAudio("fake".into()) });
}

#[tokio::test(start_paused = true)]
async fn quiet_room_is_nothing_recognized() {
    let quiet: Vec<f32> = (0..40_000).map(|i| (i as f32).sin() * 0.0005).collect();
    let h = harness(MicScript::Samples(vec![quiet]), vec![], vec![whisper_file()], FakeSink::default());
    assert_eq!(record_then_stop(&h).await, Outcome::Failed { failure: Failure::NothingRecognized });
}

#[tokio::test(start_paused = true)]
async fn a_click_is_too_short() {
    let h = harness(MicScript::Samples(vec![tone(0.1)]), vec![], vec![whisper_file()], FakeSink::default());
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    assert!(h.handle.toggle());
    assert_eq!(finished(&h.handle).await, Outcome::Failed { failure: Failure::TooShort });
}

#[tokio::test(start_paused = true)]
async fn failed_chunk_keeps_order_and_the_rest_is_used() {
    // 2 s chunks with 0.5 s overlap: 3.5 s of audio gives chunk 0 (0-2 s) and chunk 1 (1.5-3.5 s)
    let h = harness(
        MicScript::Samples(vec![tone(3.5)]),
        vec![Err(EngineError::TimedOut), Ok("後半です".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert_eq!(record_then_stop(&h).await, Outcome::Inserted);
    assert_eq!(h.sink.delivered.lock().unwrap().as_slice(), ["後半です。"]);
}

#[tokio::test(start_paused = true)]
async fn transcription_error_is_reported_when_nothing_else_worked() {
    let h = harness(
        MicScript::Samples(vec![tone(1.0)]),
        vec![Err(EngineError::ToolMissing("whisper-cli".into()))],
        vec![whisper_file()],
        FakeSink::default(),
    );
    let outcome = record_then_stop(&h).await;
    assert!(matches!(outcome, Outcome::Failed { failure: Failure::Transcription(_) }));
}

#[tokio::test(start_paused = true)]
async fn lost_device_still_transcribes_what_was_captured() {
    let h = harness(
        MicScript::SamplesThenLost(tone(1.0)),
        vec![Ok("ここまで".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert!(h.handle.toggle());
    assert_eq!(finished(&h.handle).await, Outcome::Inserted);
    assert_eq!(h.sink.delivered.lock().unwrap().as_slice(), ["ここまで。"]);
}

#[tokio::test(start_paused = true)]
async fn missing_model_and_permission_are_reported() {
    let h = harness(MicScript::Samples(vec![]), vec![], vec![], FakeSink::default());
    assert!(h.handle.toggle());
    assert_eq!(finished(&h.handle).await, Outcome::Failed { failure: Failure::ModelMissing });
    assert_eq!(h.mic.opened.load(Ordering::SeqCst), 0);

    let h = harness(MicScript::Fail(MicError::PermissionDenied), vec![], vec![whisper_file()], FakeSink::default());
    assert!(h.handle.toggle());
    assert_eq!(finished(&h.handle).await, Outcome::Failed { failure: Failure::MicPermission });
}

#[tokio::test(start_paused = true)]
async fn copy_only_delivery_is_reported() {
    let sink = FakeSink { copy_only: Some(CopyReason::SecureField), ..FakeSink::default() };
    let h = harness(MicScript::Samples(vec![tone(1.0)]), vec![Ok("秘密".into())], vec![whisper_file()], sink);
    assert_eq!(record_then_stop(&h).await, Outcome::Copied { reason: CopyReason::SecureField });
}

#[tokio::test(start_paused = true)]
async fn cancel_discards_the_recording() {
    let h = harness(
        MicScript::Samples(vec![tone(1.0)]),
        vec![Ok("消える".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    assert!(h.handle.cancel());
    wait_for(&h.handle, |p| *p == Phase::Idle).await;
    assert!(h.sink.delivered.lock().unwrap().is_empty());
    assert_eq!(h.mic.stopped.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn stops_itself_at_the_time_limit() {
    let mic = Arc::new(FakeMic {
        script: MicScript::Samples(vec![tone(1.0)]),
        opened: AtomicUsize::new(0),
        stopped: Arc::default(),
    });
    let recognizer =
        Arc::new(FakeRecognizer { replies: Mutex::new(vec![Ok("上限".into())]), prompts: Mutex::default() });
    let sink = Arc::new(FakeSink::default());
    let settings = Settings { max_recording_seconds: 3, ..Settings::default() };
    let fixed = Arc::new(Fixed { memory_mb: 8_000, models: vec![whisper_file()], settings });
    let handle = spawn(
        Services {
            microphone: mic,
            recognizer,
            llm: Arc::new(EchoLlm),
            sink: sink.clone(),
            probe: fixed.clone(),
            models: fixed.clone(),
            config: fixed,
        },
        Timing::default(),
    );
    assert!(handle.toggle());
    assert_eq!(finished(&handle).await, Outcome::Inserted);
}

#[tokio::test(start_paused = true)]
async fn audio_queued_at_stop_is_chunked_normally() {
    // 5 s of audio delivered at once, then an immediate stop: still 2 s chunks, not one 5 s chunk
    let h = harness(
        MicScript::Samples(vec![tone(5.0)]),
        vec![Ok("一".into()), Ok("二".into()), Ok("三".into()), Ok("四".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert!(h.handle.toggle());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    assert!(h.handle.toggle());
    assert_eq!(finished(&h.handle).await, Outcome::Inserted);
    assert_eq!(h.recognizer.prompts.lock().unwrap().len(), 4);
}

#[tokio::test(start_paused = true)]
async fn holding_the_shortcut_records_until_release() {
    let h = harness(
        MicScript::Samples(vec![tone(1.0), tone(0.5)]),
        vec![Ok("押している間だけ".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert!(h.handle.press());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    time::sleep(Duration::from_millis(1_500)).await;
    assert!(h.handle.release());
    assert_eq!(finished(&h.handle).await, Outcome::Inserted);
}

#[tokio::test(start_paused = true)]
async fn a_tap_keeps_recording_until_the_next_press() {
    let h = harness(
        MicScript::Samples(vec![tone(1.0), tone(0.5)]),
        vec![Ok("一度押し".into())],
        vec![whisper_file()],
        FakeSink::default(),
    );
    assert!(h.handle.press());
    wait_for(&h.handle, |p| matches!(p, Phase::Recording { .. })).await;
    time::sleep(Duration::from_millis(100)).await;
    assert!(h.handle.release());
    time::sleep(Duration::from_millis(1_500)).await;
    assert!(matches!(*h.handle.phase().borrow(), Phase::Recording { .. }));

    assert!(h.handle.press());
    assert!(h.handle.release());
    assert_eq!(finished(&h.handle).await, Outcome::Inserted);
    wait_for(&h.handle, |p| *p == Phase::Idle).await;
    // The release after the stopping press does not start anything
    assert_eq!(h.mic.opened.load(Ordering::SeqCst), 1);
}

// Real clock, as for the hung microphone above.
#[tokio::test]
async fn a_hold_released_while_the_microphone_opens_still_stops() {
    let (release_tx, release_rx) = std_mpsc::channel();
    let timing = Timing { hold_to_talk: Duration::from_millis(100), ..test_timing() };
    let h = harness_with(
        MicScript::Hang(Arc::new(Mutex::new(Some(release_rx)))),
        vec![],
        vec![whisper_file()],
        FakeSink::default(),
        timing,
    );
    assert!(h.handle.press());
    wait_for(&h.handle, |p| *p == Phase::Starting).await;
    time::sleep(Duration::from_millis(200)).await;
    assert!(h.handle.release());
    release_tx.send(()).unwrap();
    assert_eq!(finished(&h.handle).await, Outcome::Failed { failure: Failure::TooShort });
    assert_eq!(h.mic.stopped.load(Ordering::SeqCst), 1);
}
