use dictate_core::audio_level::{is_silent, peak_amplitude};

fn pcm(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[test]
fn digital_silence_has_no_peak() {
    assert_eq!(peak_amplitude(&pcm(&[0, 0, 0, 0])), 0);
}

#[test]
fn peak_is_the_largest_magnitude_regardless_of_sign() {
    assert_eq!(peak_amplitude(&pcm(&[100, -9000, 3000, -20])), 9000);
}

#[test]
fn the_most_negative_sample_does_not_overflow() {
    // -i16::MIN does not fit in an i16; computing the magnitude in i16
    // arithmetic would wrap (or panic in debug) on the loudest possible buffer.
    assert_eq!(peak_amplitude(&pcm(&[i16::MIN])), 32768);
}

#[test]
fn a_trailing_odd_byte_is_ignored_rather_than_misread() {
    let mut buffer = pcm(&[1000]);
    buffer.push(0xFF);

    assert_eq!(peak_amplitude(&buffer), 1000);
}

#[test]
fn an_empty_buffer_is_silent() {
    assert!(is_silent(&[], 200));
}

#[test]
fn silence_is_decided_at_the_threshold() {
    for (amplitude, expected) in [(0, true), (199, true), (200, false), (5000, false)] {
        assert_eq!(is_silent(&pcm(&[amplitude]), 200), expected, "amplitude {amplitude}");
    }
}

#[test]
fn a_low_noise_floor_still_counts_as_silence() {
    // What a live-but-muted input actually looks like: dither, not zeroes.
    let noise: Vec<i16> = (0..1000).map(|i| (i % 7 - 3) as i16).collect();

    assert!(is_silent(&pcm(&noise), 200));
}
