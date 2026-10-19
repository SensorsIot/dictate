use dictate_core::resample::Downsampler;

fn samples(pcm: &[u8]) -> Vec<i16> {
    pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

#[test]
fn one_second_at_48_khz_stereo_becomes_one_second_at_16_khz() {
    let mut downsampler = Downsampler::new(48_000, 2);
    let mut out = Vec::new();

    downsampler.push(&vec![0.25; 48_000 * 2], &mut out);

    assert_eq!(out.len(), 16_000 * 2);
}

#[test]
fn a_44_1_khz_input_keeps_its_duration() {
    let mut downsampler = Downsampler::new(44_100, 1);
    let mut out = Vec::new();

    // Delivered in uneven buffers, as a device does.
    for size in [512, 441, 1000, 42_147] {
        downsampler.push(&vec![0.0; size], &mut out);
    }

    assert_eq!(out.len() / 2, 16_000);
}

#[test]
fn channels_are_averaged_and_levels_scaled_to_pcm16() {
    let mut downsampler = Downsampler::new(16_000, 2);
    let mut out = Vec::new();

    downsampler.push(&[1.0, 0.0, -1.0, -1.0, 2.0, 2.0], &mut out);

    assert_eq!(samples(&out), [16_384, -32_767, 32_767]);
}

#[test]
fn a_tone_above_8_khz_is_attenuated_not_folded_into_the_speech_band() {
    // 12 kHz at 48 kHz: four samples per cycle. A bare every-third-sample
    // decimation would keep it at full scale.
    let tone: Vec<f32> = (0..4_800).map(|i| [0.0, 1.0, 0.0, -1.0][i % 4]).collect();
    let mut downsampler = Downsampler::new(48_000, 1);
    let mut out = Vec::new();

    downsampler.push(&tone, &mut out);

    let peak = samples(&out).iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    assert!(peak < 12_000, "peak {peak}");
}

#[test]
fn a_slower_input_is_upsampled_to_16_khz() {
    let mut downsampler = Downsampler::new(8_000, 1);
    let mut out = Vec::new();

    downsampler.push(&vec![0.5; 8_000], &mut out);

    assert_eq!(out.len() / 2, 16_000);
}
