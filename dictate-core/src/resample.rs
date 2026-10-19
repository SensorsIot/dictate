//! Converts a capture device's own format to the 16 kHz mono PCM16 of FR-9.1.
//!
//! A head whose audio API delivers the device's mix format — 48 kHz stereo
//! float on most Macs — feeds every buffer through one `Downsampler`. Each output
//! sample is the mean of the input samples that fall in its period: a box
//! filter, which costs nothing and keeps the aliasing of a bare decimation out of
//! the band speech recognition uses.

use crate::wav;

pub struct Downsampler {
    channels: usize,
    input_rate: u32,
    /// Output-rate units accumulated since the last emitted sample; one is
    /// emitted each time it reaches `input_rate`.
    phase: u32,
    sum: f32,
    count: u32,
}

impl Downsampler {
    pub fn new(input_rate: u32, channels: u16) -> Self {
        Self { channels: usize::from(channels.max(1)), input_rate: input_rate.max(1), phase: 0, sum: 0.0, count: 0 }
    }

    /// Appends the PCM16 little-endian bytes for `interleaved` to `out`.
    /// Samples are floats in -1.0..=1.0; anything beyond is clipped.
    pub fn push(&mut self, interleaved: &[f32], out: &mut Vec<u8>) {
        for frame in interleaved.chunks_exact(self.channels) {
            self.sum += frame.iter().sum::<f32>() / self.channels as f32;
            self.count += 1;
            self.phase += wav::SAMPLE_RATE;

            if self.phase >= self.input_rate {
                let mean = self.sum / self.count as f32;
                let sample = (mean.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;

                // A loop: an input slower than 16 kHz emits the same value
                // more than once.
                while self.phase >= self.input_rate {
                    self.phase -= self.input_rate;
                    out.extend_from_slice(&sample.to_le_bytes());
                }
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}
