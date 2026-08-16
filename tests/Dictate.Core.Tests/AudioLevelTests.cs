using System.Buffers.Binary;
using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class AudioLevelTests
{
    private static byte[] Pcm(params short[] samples)
    {
        var bytes = new byte[samples.Length * 2];
        for (var i = 0; i < samples.Length; i++)
        {
            BinaryPrimitives.WriteInt16LittleEndian(bytes.AsSpan(i * 2, 2), samples[i]);
        }

        return bytes;
    }

    [Fact]
    public void Digital_silence_has_no_peak()
    {
        Assert.Equal(0, AudioLevel.PeakAmplitude(Pcm(0, 0, 0, 0)));
    }

    [Fact]
    public void Peak_is_the_largest_magnitude_regardless_of_sign()
    {
        Assert.Equal(9000, AudioLevel.PeakAmplitude(Pcm(100, -9000, 3000, -20)));
    }

    [Fact]
    public void The_most_negative_sample_does_not_overflow()
    {
        // -short.MinValue does not fit in a short; computing the magnitude in
        // short arithmetic would wrap to a negative peak and report silence on
        // the loudest possible buffer.
        Assert.Equal(32768, AudioLevel.PeakAmplitude(Pcm(short.MinValue)));
    }

    [Fact]
    public void A_trailing_odd_byte_is_ignored_rather_than_misread()
    {
        var buffer = Pcm(1000).Concat(new byte[] { 0xFF }).ToArray();

        Assert.Equal(1000, AudioLevel.PeakAmplitude(buffer));
    }

    [Fact]
    public void An_empty_buffer_is_silent()
    {
        Assert.True(AudioLevel.IsSilent(ReadOnlySpan<byte>.Empty, 200));
    }

    [Theory]
    [InlineData(0, true)]
    [InlineData(199, true)]
    [InlineData(200, false)]
    [InlineData(5000, false)]
    public void Silence_is_decided_at_the_threshold(short amplitude, bool expected)
    {
        Assert.Equal(expected, AudioLevel.IsSilent(Pcm(amplitude), 200));
    }

    [Fact]
    public void A_low_noise_floor_still_counts_as_silence()
    {
        // What a live-but-muted input actually looks like: dither, not zeroes.
        var noise = new short[1000];
        for (var i = 0; i < noise.Length; i++)
        {
            noise[i] = (short)(i % 7 - 3);
        }

        Assert.True(AudioLevel.IsSilent(Pcm(noise), 200));
    }
}
