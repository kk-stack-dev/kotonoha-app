use super::SAMPLE_RATE;

/// How a recording is cut: fixed-length chunks that overlap so no word is lost at a boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkPlan {
    pub chunk_seconds: f64,
    pub overlap_seconds: f64,
    /// A final remainder shorter than this is a key click, not speech.
    pub min_final_seconds: f64,
}

impl Default for ChunkPlan {
    fn default() -> Self {
        Self { chunk_seconds: 30.0, overlap_seconds: 2.0, min_final_seconds: 0.3 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub index: usize,
    pub start_sample: usize,
    pub samples: Vec<f32>,
    pub is_final: bool,
}

impl Chunk {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(SAMPLE_RATE)
    }
}

/// Cuts chunks off a growing sample buffer.
#[derive(Debug)]
pub struct Chunker {
    chunk_len: usize,
    overlap_len: usize,
    min_final_len: usize,
    next_index: usize,
    last_end: usize,
}

impl Chunker {
    pub fn new(plan: ChunkPlan) -> Self {
        Self::with_rate(plan, SAMPLE_RATE)
    }

    pub fn with_rate(plan: ChunkPlan, sample_rate: u32) -> Self {
        let samples = |s: f64| (s * f64::from(sample_rate)) as usize;
        Self {
            chunk_len: samples(plan.chunk_seconds),
            overlap_len: samples(plan.overlap_seconds),
            min_final_len: samples(plan.min_final_seconds),
            next_index: 0,
            last_end: 0,
        }
    }

    fn next_start(&self) -> usize {
        if self.next_index == 0 {
            0
        } else {
            self.last_end.saturating_sub(self.overlap_len)
        }
    }

    /// Full-length chunks that became available in `buffer`.
    pub fn poll(&mut self, buffer: &[f32]) -> Vec<Chunk> {
        let mut ready = Vec::new();
        loop {
            let start = self.next_start();
            let end = start + self.chunk_len;
            if buffer.len() < end {
                return ready;
            }
            ready.push(self.cut(buffer, start, end, false));
        }
    }

    /// The remainder after recording stopped, unless it is too short to be speech.
    pub fn finish(&mut self, buffer: &[f32]) -> Option<Chunk> {
        let start = self.next_start();
        let end = buffer.len();
        if end <= start || end - start < self.min_final_len {
            return None;
        }
        Some(self.cut(buffer, start, end, true))
    }

    fn cut(&mut self, buffer: &[f32], start: usize, end: usize, is_final: bool) -> Chunk {
        let chunk =
            Chunk { index: self.next_index, start_sample: start, samples: buffer[start..end].to_vec(), is_final };
        self.next_index += 1;
        self.last_end = end;
        chunk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_chunks_and_final_remainder() {
        let plan = ChunkPlan { chunk_seconds: 10.0, overlap_seconds: 2.0, min_final_seconds: 0.3 };
        let mut chunker = Chunker::with_rate(plan, 1000);
        let mut buffer = vec![0.1; 10_000];

        let ready = chunker.poll(&buffer);
        assert_eq!(ready.len(), 1);
        assert_eq!((ready[0].start_sample, ready[0].samples.len()), (0, 10_000));

        buffer.extend(vec![0.2; 8_000]);
        let ready = chunker.poll(&buffer);
        assert_eq!(ready.len(), 1);
        assert_eq!((ready[0].index, ready[0].start_sample), (1, 8_000));

        buffer.extend(vec![0.3; 3_000]);
        let last = chunker.finish(&buffer).unwrap();
        assert_eq!((last.start_sample, last.samples.len(), last.is_final), (16_000, 5_000, true));
    }

    #[test]
    fn short_remainder_is_not_a_chunk() {
        let mut chunker = Chunker::new(ChunkPlan::default());
        assert!(chunker.finish(&[0.0; 1_000]).is_none());
        assert!(chunker.finish(&[]).is_none());
    }
}
