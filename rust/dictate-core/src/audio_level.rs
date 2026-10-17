//! Cheap signal checks on a 16-bit PCM buffer.
//!
//! Exists because a capture device can hand back perfectly well-formed frames
//! that are digital silence — a USB interface that has not finished spinning its
//! capture stream up, a muted input, the wrong device selected. Uploading that
//! costs money and comes back as "nothing was recognised", which points the
//! blame at the speech model instead of the microphone.

/// Largest absolute sample in the buffer, 0–32768. A trailing odd byte is
/// ignored rather than misread as half a sample.
pub fn peak_amplitude(pcm16: &[u8]) -> i32 {
    pcm16
        .chunks_exact(2)
        // Widen before abs: i16::MIN has no positive i16.
        .map(|pair| i32::from(i16::from_le_bytes([pair[0], pair[1]])).abs())
        .max()
        .unwrap_or(0)
}

/// True when nothing in the buffer rises above `threshold`. The default of 200
/// is about -44 dBFS: comfortably above a noise floor, far below anything anyone
/// would call speech.
pub fn is_silent(pcm16: &[u8], threshold: i32) -> bool {
    peak_amplitude(pcm16) < threshold
}
