//! Microphone capture on a dedicated thread that owns the cpal stream.
//!
//! The stream is rebuilt on the current default input device when it is invalidated (for example when
//! a Bluetooth headset switches to its microphone profile and the sample rate changes), or when no audio
//! has arrived for a while. Every buffer is resampled from the rate of the stream that produced it.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, Sample, SampleFormat, SizedSample};
use kotonoha_core::ports::{AudioEvent, MicError, MicInfo, MicStream, Microphone};
use tokio::sync::mpsc::UnboundedSender;

use super::permission::{request_microphone_permission, MicPermission};
use super::resample::{mixdown, To16k};

/// A running stream that delivers nothing for this long is rebuilt.
const STALL_TIMEOUT: Duration = Duration::from_secs(2);
/// Consecutive rebuilds without audio before giving up.
const MAX_REBUILDS: u32 = 3;
const POLL: Duration = Duration::from_millis(200);
/// How long opening waits for the user to answer the first permission prompt.
const PERMISSION_PROMPT_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct CpalMicrophone;

impl Microphone for CpalMicrophone {
    fn open(&self, events: UnboundedSender<AudioEvent>) -> Result<(Box<dyn MicStream>, MicInfo), MicError> {
        if request_microphone_permission(PERMISSION_PROMPT_WAIT) != MicPermission::Granted {
            return Err(MicError::PermissionDenied);
        }
        let (control_tx, control_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_control = control_tx.clone();
        let thread = std::thread::Builder::new()
            .name("kotonoha-audio".into())
            .spawn(move || run_capture(control_rx, thread_control, events, ready_tx))
            .map_err(|e| MicError::Failed(format!("could not start audio thread: {e}")))?;
        match ready_rx.recv() {
            Ok(Ok(info)) => Ok((Box::new(CaptureHandle { control: control_tx, thread: Some(thread) }), info)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(MicError::Failed("audio thread exited".into())),
        }
    }
}

struct CaptureHandle {
    control: mpsc::Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl MicStream for CaptureHandle {
    fn stop(self: Box<Self>) {}
}

/// Dropping the handle stops capture, including when an abandoned (timed-out) open finishes late.
impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.control.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Message {
    Data { generation: u64, samples: Vec<f32> },
    Error { generation: u64, kind: ErrorKind, message: String },
    Stop,
}

struct Active {
    _stream: cpal::Stream,
    resampler: To16k,
    info: MicInfo,
}

fn run_capture(
    control: mpsc::Receiver<Message>,
    control_tx: mpsc::Sender<Message>,
    events: UnboundedSender<AudioEvent>,
    ready: mpsc::Sender<Result<MicInfo, MicError>>,
) {
    let mut generation = 0;
    let mut active = match build(generation, &control_tx) {
        Ok(active) => {
            let _ = ready.send(Ok(active.info.clone()));
            active
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut last_audio = Instant::now();
    let mut rebuilds = 0;

    loop {
        let mut rebuild_reason = None;
        match control.recv_timeout(POLL) {
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Message::Data { generation: g, samples }) if g == generation => {
                last_audio = Instant::now();
                rebuilds = 0;
                let converted = active.resampler.push(&samples);
                if !converted.is_empty() && events.send(AudioEvent::Samples(converted)).is_err() {
                    return;
                }
            }
            Ok(Message::Error { generation: g, kind, message }) if g == generation => match kind {
                // The stream keeps running in these cases
                ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied => {
                    tracing::info!(?kind, %message, "audio stream notice");
                }
                _ => rebuild_reason = Some(message),
            },
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
        if rebuild_reason.is_none() && last_audio.elapsed() > STALL_TIMEOUT {
            rebuild_reason = Some(format!("no audio for {STALL_TIMEOUT:?}"));
        }
        let Some(reason) = rebuild_reason else { continue };

        if rebuilds >= MAX_REBUILDS {
            let _ = events.send(AudioEvent::Lost(reason));
            return;
        }
        rebuilds += 1;
        generation += 1;
        tracing::warn!(%reason, attempt = rebuilds, "rebuilding audio input");
        last_audio = Instant::now();
        match build(generation, &control_tx) {
            Ok(next) => {
                let device = next.info.device.clone();
                tracing::info!(%device, rate = next.info.sample_rate, "audio input rebuilt");
                active = next;
                let _ = events.send(AudioEvent::Restarted { device });
            }
            Err(e) => tracing::warn!(error = %e, "could not rebuild audio input; will retry"),
        }
    }
}

/// Opens the current default input device with its native format.
fn build(generation: u64, control: &mpsc::Sender<Message>) -> Result<Active, MicError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(MicError::NoDevice)?;
    let name = device.description().map_or_else(|_| "unknown".to_owned(), |d| d.name().to_owned());
    let config = device.default_input_config().map_err(map_error)?;
    let channels = usize::from(config.channels());
    let rate = config.sample_rate();
    let format = config.sample_format();

    let errors = control.clone();
    let on_error = move |e: cpal::Error| {
        let _ = errors.send(Message::Error { generation, kind: e.kind(), message: e.to_string() });
    };
    let config = config.into();
    let stream = match format {
        SampleFormat::F32 => input_stream::<f32>(&device, &config, channels, generation, control.clone(), on_error),
        SampleFormat::I16 => input_stream::<i16>(&device, &config, channels, generation, control.clone(), on_error),
        SampleFormat::I32 => input_stream::<i32>(&device, &config, channels, generation, control.clone(), on_error),
        SampleFormat::U16 => input_stream::<u16>(&device, &config, channels, generation, control.clone(), on_error),
        other => return Err(MicError::Failed(format!("unsupported sample format {other}"))),
    }?;
    stream.play().map_err(map_error)?;
    let resampler = To16k::new(rate).map_err(MicError::Failed)?;
    Ok(Active { _stream: stream, resampler, info: MicInfo { device: name, sample_rate: rate } })
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    generation: u64,
    data: mpsc::Sender<Message>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, MicError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            *config,
            move |input: &[T], _: &_| {
                let samples: Vec<f32> = input.iter().map(|&s| f32::from_sample(s)).collect();
                let _ = data.send(Message::Data { generation, samples: mixdown(&samples, channels) });
            },
            on_error,
            None,
        )
        .map_err(map_error)
}

fn map_error(e: cpal::Error) -> MicError {
    match e.kind() {
        ErrorKind::PermissionDenied => MicError::PermissionDenied,
        ErrorKind::DeviceNotAvailable => MicError::NoDevice,
        _ => MicError::Failed(e.to_string()),
    }
}
