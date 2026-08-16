using NAudio.Wave;

namespace Dictate.Windows;

/// <summary>
/// Short synthesised tones for start, stop and error.
///
/// Not SystemSounds: those are the Windows notification and error chimes, which
/// are long, loud, and carry the wrong meaning — an error chime every time you
/// finish a sentence trains you to ignore it. These are three short tones that
/// differ only in pitch, so the cue is legible without being an event.
///
/// The output device is opened once and kept open, feeding silence. Opening it
/// per beep would put device-open latency between the key press and the cue —
/// which is the exact bug this class is being changed to help diagnose.
/// </summary>
internal sealed class Feedback : IDisposable
{
    private const int SampleRate = 44_100;

    private readonly bool _enabled;
    private readonly byte[] _start;
    private readonly byte[] _stop;
    private readonly byte[] _error;

    private WaveOutEvent? _output;
    private BufferedWaveProvider? _sink;

    internal Feedback(bool enabled)
    {
        _enabled = enabled;

        // Rising for "listening", a lower falling note for "done". A major
        // sixth apart, so they are unmistakable even at low volume.
        _start = Tone(988, 70);   // B5
        _stop = Tone(587, 70);    // D5
        _error = Tone(196, 180);  // G3 — clearly not one of the other two

        if (_enabled)
        {
            OpenOutput();
        }
    }

    private void OpenOutput()
    {
        try
        {
            _sink = new BufferedWaveProvider(new WaveFormat(SampleRate, 16, 1))
            {
                // Small: this only ever holds one short tone. A large buffer
                // would let beeps queue up behind each other.
                BufferDuration = TimeSpan.FromMilliseconds(600),
                DiscardOnBufferOverflow = true,
            };

            _output = new WaveOutEvent { DesiredLatency = 100 };
            _output.Init(_sink);
            _output.Play(); // plays silence until something is written
        }
        catch (Exception)
        {
            // No output device, or one that refuses these settings. Dictation
            // works fine without cues; failing here must not stop startup.
            _output?.Dispose();
            _output = null;
            _sink = null;
        }
    }

    /// <summary>
    /// A sine tone with a short raised-cosine fade at each end. Without the
    /// fade the abrupt start and stop produce an audible click that is louder
    /// than the tone itself.
    /// </summary>
    private static byte[] Tone(double frequency, int milliseconds, double amplitude = 0.22)
    {
        var samples = SampleRate * milliseconds / 1000;
        var buffer = new byte[samples * 2];
        var fade = Math.Min(samples / 2, SampleRate * 6 / 1000); // 6 ms

        for (var i = 0; i < samples; i++)
        {
            var envelope = 1.0;
            if (i < fade)
            {
                envelope = 0.5 * (1 - Math.Cos(Math.PI * i / fade));
            }
            else if (i >= samples - fade)
            {
                envelope = 0.5 * (1 - Math.Cos(Math.PI * (samples - 1 - i) / fade));
            }

            var value = Math.Sin(2 * Math.PI * frequency * i / SampleRate) * amplitude * envelope;
            var sample = (short)(value * short.MaxValue);

            buffer[i * 2] = (byte)(sample & 0xFF);
            buffer[i * 2 + 1] = (byte)((sample >> 8) & 0xFF);
        }

        return buffer;
    }

    private void Play(byte[] tone)
    {
        if (!_enabled || _sink is null)
        {
            return;
        }

        try
        {
            // Drop anything still pending so a fast press-release does not hear
            // the start tone after the stop tone.
            _sink.ClearBuffer();
            _sink.AddSamples(tone, 0, tone.Length);
        }
        catch (Exception)
        {
            // A device removed while running. Silence is an acceptable outcome.
        }
    }

    internal void Start() => Play(_start);

    internal void Stop() => Play(_stop);

    internal void Error() => Play(_error);

    public void Dispose()
    {
        try
        {
            _output?.Stop();
        }
        catch (Exception)
        {
            // Disposing a device that is already gone.
        }

        _output?.Dispose();
        _output = null;
        _sink = null;
    }
}
