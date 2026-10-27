//! Runs real sessions from the command line: a WAV file stands in for the microphone, or the real
//! microphone records for a fixed time. Results are printed instead of pasted.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use kotonoha_core::dictionary::Dictionary;
use kotonoha_core::ports::{
    AudioEvent, ConfigSource, Delivery, DeliveryOptions, MicError, MicInfo, MicStream, Microphone, TextSink,
};
use kotonoha_core::session::{self, Outcome, Phase, Services, Timing};
use kotonoha_core::settings::Settings;
use kotonoha_platform::audio::{mixdown, CpalMicrophone, To16k};
use kotonoha_platform::library::ModelDirectory;
use kotonoha_platform::llama::LlamaCli;
use kotonoha_platform::store::ConfigStore;
use kotonoha_platform::system::MemoryProbe;
use kotonoha_platform::whisper::WhisperCli;
use tokio::sync::mpsc::UnboundedSender;

pub async fn transcribe_file(store: Arc<ConfigStore>, file: &Path, no_llm: bool) -> Result<()> {
    let samples = read_wav_16k(file)?;
    let seconds = samples.len() as f64 / 16_000.0;
    eprintln!("{}: {seconds:.1} s of audio", file.display());
    let config: Arc<dyn ConfigSource> = if no_llm { Arc::new(NoLlm(store.clone())) } else { store.clone() };
    let outcome = run_session(&store, config, Arc::new(FileMicrophone { samples }), None).await;
    report(outcome)
}

pub async fn record(store: Arc<ConfigStore>, seconds: u64) -> Result<()> {
    eprintln!("Recording for {seconds} s from the default input device...");
    let outcome =
        run_session(&store, store.clone(), Arc::new(CpalMicrophone), Some(Duration::from_secs(seconds))).await;
    report(outcome)
}

async fn run_session(
    store: &Arc<ConfigStore>,
    config: Arc<dyn ConfigSource>,
    microphone: Arc<dyn Microphone>,
    record_for: Option<Duration>,
) -> Outcome {
    let settings = store.settings();
    let models_store = store.clone();
    let services = Services {
        microphone,
        recognizer: Arc::new(WhisperCli::new(settings.tools.whisper.clone(), Some(store.dirs().tools_dir()))),
        llm: Arc::new(LlamaCli::new(settings.tools.llama.clone(), Some(store.dirs().tools_dir()))),
        sink: Arc::new(PrintSink),
        probe: Arc::new(MemoryProbe::default()),
        models: Arc::new(ModelDirectory::new(move || models_store.models_dir())),
        config,
    };
    let handle = session::spawn(services, Timing::default());
    let mut phase = handle.phase();
    handle.toggle();
    let _ = phase.wait_for(|p| !matches!(p, Phase::Idle | Phase::Starting)).await;
    if matches!(*phase.borrow(), Phase::Recording { .. }) {
        if let Some(duration) = record_for {
            tokio::time::sleep(duration).await;
        }
        handle.toggle();
    }
    let finished = phase.wait_for(|p| matches!(p, Phase::Finished { .. })).await.map(|p| p.clone());
    match finished {
        Ok(Phase::Finished { outcome }) => outcome,
        _ => unreachable!("controller stopped"),
    }
}

fn report(outcome: Outcome) -> Result<()> {
    match outcome {
        Outcome::Inserted | Outcome::Copied { .. } => Ok(()),
        Outcome::Failed { failure } => anyhow::bail!("failed: {failure:?}"),
    }
}

/// Reads any WAV file as 16 kHz mono.
pub fn read_wav_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
        }
    };
    let mono = mixdown(&interleaved, usize::from(spec.channels));
    let mut resampler = To16k::new(spec.sample_rate).map_err(anyhow::Error::msg)?;
    // Pad so the resampler's last block is flushed
    let mut out = resampler.push(&mono);
    out.extend(resampler.push(&vec![0.0; spec.sample_rate as usize / 2]));
    Ok(out)
}

/// Delivers a file's samples as if a microphone had captured them.
struct FileMicrophone {
    samples: Vec<f32>,
}

struct Idle(#[allow(dead_code)] UnboundedSender<AudioEvent>);

impl MicStream for Idle {
    fn stop(self: Box<Self>) {}
}

impl Microphone for FileMicrophone {
    fn open(&self, events: UnboundedSender<AudioEvent>) -> Result<(Box<dyn MicStream>, MicInfo), MicError> {
        for block in self.samples.chunks(1_600) {
            let _ = events.send(AudioEvent::Samples(block.to_vec()));
        }
        Ok((Box::new(Idle(events)), MicInfo { device: "file".into(), sample_rate: 16_000 }))
    }
}

struct PrintSink;

#[async_trait]
impl TextSink for PrintSink {
    async fn deliver(&self, text: &str, _: DeliveryOptions) -> Delivery {
        println!("{text}");
        Delivery::Inserted
    }
}

struct NoLlm(Arc<ConfigStore>);

impl ConfigSource for NoLlm {
    fn settings(&self) -> Settings {
        Settings { llm_rewrite: false, ..self.0.settings() }
    }
    fn dictionary(&self) -> Dictionary {
        self.0.dictionary()
    }
}
