//! Wraps raw PCM in a RIFF/WAVE container. Scribe accepts bare audio uploads,
//! but a well-formed header removes any guessing about sample rate and channel
//! count, and costs 44 bytes.

use std::time::Duration;

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;

const HEADER_BYTES: usize = 44;
const BYTE_RATE: u32 = SAMPLE_RATE * CHANNELS as u32 * BITS_PER_SAMPLE as u32 / 8;
const BLOCK_ALIGN: u16 = CHANNELS * BITS_PER_SAMPLE / 8;

pub fn from_pcm16(pcm: &[u8]) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut buffer = Vec::with_capacity(HEADER_BYTES + pcm.len());

    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_len).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");

    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    buffer.extend_from_slice(&1u16.to_le_bytes()); // format: PCM
    buffer.extend_from_slice(&CHANNELS.to_le_bytes());
    buffer.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buffer.extend_from_slice(&BYTE_RATE.to_le_bytes());
    buffer.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
    buffer.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_len.to_le_bytes());

    buffer.extend_from_slice(pcm);
    buffer
}

/// How long the given PCM buffer plays for.
pub fn duration_of(pcm_byte_count: usize) -> Duration {
    Duration::from_secs_f64(pcm_byte_count as f64 / f64::from(BYTE_RATE))
}
