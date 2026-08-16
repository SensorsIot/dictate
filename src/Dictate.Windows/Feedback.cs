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
/// The output device is opened on the first tone and closed again after 30
/// seconds of quiet. Two failure modes are being avoided at once: opening it per
/// beep would put device-open latency between the key press and the cue, while
/// holding it open for the life of the process — which is what this class did
/// first — means dictate keeps a stream on the speakers around the clock,
/// stopping the endpoint idling and interfering with whatever else uses it.
/// Within a dictation every beep is instant; between them dictate is not there.
/// </summary>
internal sealed class Feedback : IDisposable
{
    private const int SampleRate = 44_100;

    private readonly bool _enabled;
    private readonly byte[] _start;
    private readonly byte[] _stop;
    private readonly byte[] _error;

    /// <summary>
    /// How long the output device stays open after the last tone. Long enough
    /// that every beep within a dictation is instant, short enough that dictate
    /// is not holding a stream on the speakers while idle.
    /// </summary>
    private static readonly TimeSpan IdleTimeout = TimeSpan.FromSeconds(30);

    private readonly object _gate = new();

    private WaveOutEvent? _output;
    private BufferedWaveProvider? _sink;
    private System.Threading.Timer? _idle;

    internal Feedback(bool enabled)
    {
        _enabled = enabled;

        // Rising for "listening", a lower falling note for "done". A major
        // sixth apart, so they are unmistakable even at low volume.
        _start = Tone(988, 70);   // B5
        _stop = Tone(587, 70);    // D5
        _error = Tone(196, 180);  // G3 — clearly not one of the other two
    }

    private void OpenOutput()
    {
        if (_output is not null)
        {
            return;
        }

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
        if (!_enabled)
        {
            return;
        }

        lock (_gate)
        {
            OpenOutput();

            if (_sink is null)
            {
                return;
            }

            try
            {
                // Drop anything still pending so a fast press-release does not
                // hear the start tone after the stop tone.
                _sink.ClearBuffer();
                _sink.AddSamples(tone, 0, tone.Length);
            }
            catch (Exception)
            {
                // A device removed while running. Silence is acceptable.
            }

            // Restart the idle countdown: the device closes only once dictation
            // has actually stopped for a while.
            _idle ??= new System.Threading.Timer(_ => CloseIfIdle(), null, Timeout.Infinite, Timeout.Infinite);
            _idle.Change(IdleTimeout, Timeout.InfiniteTimeSpan);
        }
    }

    private void CloseIfIdle()
    {
        lock (_gate)
        {
            CloseOutput();
        }
    }

    private void CloseOutput()
    {
        try
        {
            _output?.Stop();
        }
        catch (Exception)
        {
            // Device already gone.
        }

        _output?.Dispose();
        _output = null;
        _sink = null;
    }

    internal void Start() => Play(_start);

    internal void Stop() => Play(_stop);

    internal void Error() => Play(_error);

    public void Dispose()
    {
        lock (_gate)
        {
            _idle?.Dispose();
            _idle = null;
            CloseOutput();
        }
    }
}
