using System.Buffers.Binary;

namespace Dictate.Core;

/// <summary>
/// Wraps raw PCM in a RIFF/WAVE container. Scribe accepts bare audio uploads,
/// but a well-formed header removes any guessing about sample rate and channel
/// count, and costs 44 bytes.
/// </summary>
public static class Wav
{
    public const int SampleRate = 16_000;
    public const int Channels = 1;
    public const int BitsPerSample = 16;

    private const int HeaderBytes = 44;

    public static byte[] FromPcm16(ReadOnlySpan<byte> pcm)
    {
        var buffer = new byte[HeaderBytes + pcm.Length];
        var span = buffer.AsSpan();

        var byteRate = SampleRate * Channels * BitsPerSample / 8;
        var blockAlign = Channels * BitsPerSample / 8;

        "RIFF"u8.CopyTo(span[..4]);
        BinaryPrimitives.WriteUInt32LittleEndian(span[4..8], (uint)(36 + pcm.Length));
        "WAVE"u8.CopyTo(span[8..12]);

        "fmt "u8.CopyTo(span[12..16]);
        BinaryPrimitives.WriteUInt32LittleEndian(span[16..20], 16);           // PCM chunk size
        BinaryPrimitives.WriteUInt16LittleEndian(span[20..22], 1);            // format: PCM
        BinaryPrimitives.WriteUInt16LittleEndian(span[22..24], Channels);
        BinaryPrimitives.WriteUInt32LittleEndian(span[24..28], SampleRate);
        BinaryPrimitives.WriteUInt32LittleEndian(span[28..32], (uint)byteRate);
        BinaryPrimitives.WriteUInt16LittleEndian(span[32..34], (ushort)blockAlign);
        BinaryPrimitives.WriteUInt16LittleEndian(span[34..36], BitsPerSample);

        "data"u8.CopyTo(span[36..40]);
        BinaryPrimitives.WriteUInt32LittleEndian(span[40..44], (uint)pcm.Length);

        pcm.CopyTo(span[HeaderBytes..]);
        return buffer;
    }

    /// <summary>How long the given PCM buffer plays for.</summary>
    public static TimeSpan DurationOf(int pcmByteCount) =>
        TimeSpan.FromSeconds((double)pcmByteCount / (SampleRate * Channels * BitsPerSample / 8));
}
