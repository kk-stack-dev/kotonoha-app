/// Peak RMS over 50 ms windows below which a chunk is silence. Whisper invents stock phrases on
/// silent input, so silent chunks are never sent to it.
pub const SILENCE_THRESHOLD: f32 = 0.002;
const WINDOW: usize = 800;

/// Loudest 50 ms window (RMS) of `samples`.
pub fn peak_rms(samples: &[f32]) -> f32 {
    samples.chunks(WINDOW).map(|w| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt()).fold(0.0, f32::max)
}

pub fn is_silent(samples: &[f32]) -> bool {
    peak_rms(samples) < SILENCE_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gate() {
        assert!(is_silent(&[0.0; 16_000]));
        let tone: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.1).sin() * 0.1).collect();
        assert!(!is_silent(&tone));
        assert_eq!(peak_rms(&[]), 0.0);
    }
}
