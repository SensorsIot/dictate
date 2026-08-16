using System.Buffers.Binary;

namespace Dictate.Core;

/// <summary>
/// Cheap signal checks on a 16-bit PCM buffer.
///
/// Exists because a capture device can hand back perfectly well-formed frames
/// that are digital silence — a USB interface that has not finished spinning its
/// capture stream up, a muted input, the wrong device selected. Uploading that
/// costs money and comes back as "nothing was recognised", which points the
/// blame at the speech model instead of the microphone.
/// </summary>
public static class AudioLevel
{
    /// <summary>
    /// Largest absolute sample in the buffer, 0–32767. A trailing odd byte is
    /// ignored rather than misread as half a sample.
    /// </summary>
    public static int PeakAmplitude(ReadOnlySpan<byte> pcm16)
    {
        var peak = 0;

        for (var i = 0; i + 1 < pcm16.Length; i += 2)
        {
            var sample = BinaryPrimitives.ReadInt16LittleEndian(pcm16.Slice(i, 2));

            // Negate via int: -short.MinValue overflows a short.
            var magnitude = sample < 0 ? -(int)sample : sample;
            if (magnitude > peak)
            {
                peak = magnitude;
            }
        }

        return peak;
    }

    /// <summary>
    /// True when nothing in the buffer rises above <paramref name="threshold"/>.
    /// The default of 200 is about -44 dBFS: comfortably above a noise floor,
    /// far below anything anyone would call speech.
    /// </summary>
    public static bool IsSilent(ReadOnlySpan<byte> pcm16, int threshold) =>
        PeakAmplitude(pcm16) < threshold;
}
