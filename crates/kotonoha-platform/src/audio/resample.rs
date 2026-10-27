//! Converts captured audio to the 16 kHz mono that Whisper expects.

use kotonoha_core::audio::SAMPLE_RATE;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

const CHUNK_FRAMES: usize = 1_024;

/// Streaming resampler for one capture stream. Band-limited (FFT), so higher frequencies fold
/// into the speech band far less than with plain interpolation.
pub struct To16k {
    resampler: Option<Fft<f32>>,
    pending: Vec<f32>,
    scratch: Vec<f32>,
}

impl To16k {
    pub fn new(input_rate: u32) -> Result<Self, String> {
        let resampler = if input_rate == SAMPLE_RATE {
            None
        } else {
            Some(
                Fft::new(input_rate as usize, SAMPLE_RATE as usize, CHUNK_FRAMES, 1, FixedSync::Input)
                    .map_err(|e| format!("cannot resample from {input_rate} Hz: {e}"))?,
            )
        };
        Ok(Self { resampler, pending: Vec::new(), scratch: Vec::new() })
    }

    /// Feeds mono samples and returns whatever 16 kHz output is ready.
    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        let Some(resampler) = self.resampler.as_mut() else {
            return input.to_vec();
        };
        self.pending.extend_from_slice(input);
        let mut output = Vec::new();
        loop {
            let needed = resampler.input_frames_next();
            if self.pending.len() < needed {
                return output;
            }
            let max_out = resampler.output_frames_max();
            self.scratch.resize(max_out, 0.0);
            let input =
                InterleavedSlice::new(&self.pending[..needed], 1, needed).expect("sized from input_frames_next");
            let mut out =
                InterleavedSlice::new_mut(&mut self.scratch, 1, max_out).expect("sized from output_frames_max");
            match resampler.process_into_buffer(&input, &mut out, None) {
                Ok((used, written)) => {
                    output.extend_from_slice(&self.scratch[..written]);
                    self.pending.drain(..used);
                }
                Err(e) => {
                    tracing::error!(error = %e, "resampling failed; dropping audio block");
                    self.pending.clear();
                    return output;
                }
            }
        }
    }
}

/// Averages interleaved frames into one channel.
pub fn mixdown(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_16k_through() {
        let mut r = To16k::new(16_000).unwrap();
        assert_eq!(r.push(&[0.1, 0.2]), vec![0.1, 0.2]);
    }

    #[test]
    fn converts_48k_and_44k_to_about_16k_per_second() {
        for rate in [48_000, 44_100] {
            let mut r = To16k::new(rate).unwrap();
            let tone: Vec<f32> =
                (0..rate).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
            // Fed in uneven pieces, as a device delivers them
            let produced: usize = tone.chunks(777).map(|c| r.push(c).len()).sum();
            assert!((14_500..=16_000).contains(&produced), "{rate} Hz -> {produced} samples");
        }
    }

    #[test]
    fn mixes_stereo_to_mono() {
        assert_eq!(mixdown(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(mixdown(&[0.3], 1), vec![0.3]);
    }
}
