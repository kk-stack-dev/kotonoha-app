use super::SAMPLE_RATE;

/// Samples captured in one session. Appends past the capacity are dropped (the session
/// stops itself at the maximum duration long before that happens).
#[derive(Debug, Default)]
pub struct Recording {
    samples: Vec<f32>,
    capacity: usize,
}

impl Recording {
    pub fn with_max_seconds(seconds: f64) -> Self {
        Self { samples: Vec::new(), capacity: (seconds * f64::from(SAMPLE_RATE)) as usize }
    }

    /// Appends samples and returns how many were dropped because the recording is full.
    pub fn append(&mut self, samples: &[f32]) -> usize {
        let room = self.capacity.saturating_sub(self.samples.len());
        let taken = samples.len().min(room);
        self.samples.extend_from_slice(&samples[..taken]);
        samples.len() - taken
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(SAMPLE_RATE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_cap() {
        let mut rec = Recording::with_max_seconds(1.0);
        assert_eq!(rec.append(&vec![0.5; 10_000]), 0);
        assert_eq!(rec.append(&vec![0.5; 10_000]), 4_000);
        assert_eq!(rec.len(), 16_000);
        assert!((rec.seconds() - 1.0).abs() < 1e-9);
    }
}
