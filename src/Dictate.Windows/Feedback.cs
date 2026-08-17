using System.Diagnostics;
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
    /// How long the output device stays open after the last tone. See
    /// <see cref="DictateConfig.FeedbackIdleSeconds"/> — this is load-bearing for
    /// audio capture, not just for how promptly a beep arrives.
    /// </summary>
    private readonly TimeSpan _idleTimeout;

    private readonly object _gate = new();

    private WaveOutEvent? _output;
    private BufferedWaveProvider? _sink;
    private System.Threading.Timer? _idle;

    /// <summary>
    /// Raised with how long an output-device open actually took. Opening the
    /// device is the one unbounded cost in here and it was invisible until it
    /// was measured — on one machine it ran to 3.2 s.
    /// </summary>
    public event Action<TimeSpan>? DeviceOpened;

    /// <summary>
    /// Opens the output device now, off the caller's thread, so the first tone
    /// does not pay for it.
    ///
    /// The idle countdown is armed exactly as a real tone would arm it, so this
    /// does not quietly reintroduce what O-08 fixed: if no dictation follows
    /// within the timeout the device closes again and dictate goes back to not
    /// holding a stream on the speakers.
    /// </summary>
    internal void PreWarm()
    {
        if (!_enabled)
        {
            return;
        }

        _ = Task.Run(() =>
        {
            lock (_gate)
            {
                OpenOutput();
                ArmIdle();
            }
        });
    }

    internal Feedback(bool enabled, int idleSeconds)
    {
        _enabled = enabled;
        _idleTimeout = TimeSpan.FromSeconds(Math.Max(5, idleSeconds));

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

        var started = Stopwatch.GetTimestamp();

        try
        {
            _sink = new BufferedWaveProvider(new WaveFormat(SampleRate, 16, 1))
            {
                // Small: this only ever holds one short tone. A large buffer
                // would let beeps queue up behind each other.
                BufferDuration = TimeSpan.FromMilliseconds(600),
                DiscardOnBufferOverflow = true,

                // Pad short reads with silence rather than returning fewer bytes
                // than asked for. Without this the output driver is handed a
                // partially-filled buffer between tones and plays whatever was
                // there before — heard as a burst of noise around the beep.
                // It defaults to true, but this is load-bearing enough to state.
                ReadFully = true,
            };

            // 60 ms rather than 100: a 70 ms tone was less than one buffer
            // period, so it straddled a boundary and part of it could be cut.
            _output = new WaveOutEvent { DesiredLatency = 60, NumberOfBuffers = 3 };
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

        DeviceOpened?.Invoke(Stopwatch.GetElapsedTime(started));
    }

    private void ArmIdle()
    {
        // Restart the idle countdown: the device closes only once dictation
        // has actually stopped for a while.
        _idle ??= new System.Threading.Timer(_ => CloseIfIdle(), null, Timeout.Infinite, Timeout.Infinite);
        _idle.Change(_idleTimeout, Timeout.InfiniteTimeSpan);
    }

    /// <summary>
    /// A sine tone with a short raised-cosine fade at each end. Without the
    /// fade the abrupt start and stop produce an audible click that is louder
    /// than the tone itself.
    /// </summary>
    private static byte[] Tone(double frequency, int milliseconds, double amplitude = 0.22)
    {
        var samples = SampleRate * milliseconds / 1000;

        // Trailing silence, so playback ends on quiet rather than on the buffer
        // running dry mid-stream. An underrun at the tail is heard as a click or
        // a fragment of the previous tone.
        var tail = SampleRate * 80 / 1000;
        var buffer = new byte[(samples + tail) * 2];
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

    /// <summary>
    /// Hands the tone to a worker and returns immediately.
    ///
    /// This is called from the session state machine on the UI thread, and
    /// <see cref="OpenOutput"/> can take seconds when the device has closed
    /// itself after the idle timeout. Blocking the message loop there freezes
    /// the whole application: hotkey events reach it through
    /// <c>BeginInvoke</c>, so the user's key-*release* cannot be processed until
    /// the beep finishes opening a sound device. A late tone is cosmetic; a
    /// frozen pump is not.
    /// </summary>
    private void Play(byte[] tone)
    {
        if (!_enabled)
        {
            return;
        }

        _ = Task.Run(() => PlayCore(tone));
    }

    private void PlayCore(byte[] tone)
    {
        lock (_gate)
        {
            OpenOutput();

            if (_sink is null)
            {
                return;
            }

            try
            {
                // Clear first. 0.1.7 dropped this on the reasoning that the two
                // tones "cannot overlap in practice" — wrong: without it queued
                // tones accumulate and play back in sequence, heard as the beep
                // repeating. Never more than one tone pending.
                _sink.ClearBuffer();
                _sink.AddSamples(tone, 0, tone.Length);
            }
            catch (Exception)
            {
                // A device removed while running. Silence is acceptable.
            }

            ArmIdle();
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
