using System.Diagnostics;
using NAudio.CoreAudioApi;
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
/// **Output goes through WASAPI, deliberately, while capture goes through
/// winmm.** They must not share a layer: opening a winmm output device stalls a
/// live winmm capture stream for as long as the open takes, which is seconds on
/// a machine with many endpoints, and the stall is paid in lost audio at the
/// start of the utterance. Keeping the two on different APIs removes the
/// contention rather than scheduling around it.
///
/// WASAPI is right for output for the same reason it is wrong for capture. In
/// shared mode the device dictates the format, so capture would need a resampler
/// in the hot path — but these tones are synthesised, so they are simply
/// generated in whatever format the device asks for, once, when it opens.
///
/// The device is opened at startup and closed again after
/// <see cref="DictateConfig.FeedbackIdleSeconds"/> of quiet, so dictate is not
/// holding a stream on the speakers around the clock.
/// </summary>
internal sealed class Feedback : IDisposable
{
    // Rising for "listening", a lower falling note for "done". A major sixth
    // apart, so they are unmistakable even at low volume. The error note is
    // clearly not one of the other two.
    private static readonly (double Hz, int Ms) StartTone = (988, 70);   // B5
    private static readonly (double Hz, int Ms) StopTone = (587, 70);    // D5
    private static readonly (double Hz, int Ms) ErrorTone = (196, 180);  // G3

    private readonly bool _enabled;

    // Rendered when the device opens, in that device's own format.
    private byte[]? _start;
    private byte[]? _stop;
    private byte[]? _error;

    /// <summary>
    /// How long the output device stays open after the last tone. See
    /// <see cref="DictateConfig.FeedbackIdleSeconds"/> — this is load-bearing for
    /// audio capture, not just for how promptly a beep arrives.
    /// </summary>
    private readonly TimeSpan _idleTimeout;

    private readonly object _gate = new();

    private WasapiOut? _output;
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
    /// does not turn into holding the device open indefinitely: if no dictation
    /// follows within the timeout it closes again, and dictate goes back to not
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
            // 60 ms rather than 100: a 70 ms tone is less than one buffer period
            // at 100, so it straddles a boundary and part of it can be cut.
            _output = new WasapiOut(AudioClientShareMode.Shared, false, 60);

            // Shared mode means the device names the format and we meet it —
            // which costs nothing here, because the tones have not been rendered
            // yet and are synthesised straight into it.
            var format = _output.OutputWaveFormat;

            _sink = new BufferedWaveProvider(format)
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

            _output.Init(_sink);
            _output.Play(); // plays silence until something is written

            _start = Tone(format, StartTone.Hz, StartTone.Ms);
            _stop = Tone(format, StopTone.Hz, StopTone.Ms);
            _error = Tone(format, ErrorTone.Hz, ErrorTone.Ms);
        }
        catch (Exception)
        {
            // No output device, or one whose format we cannot render. Dictation
            // works fine without cues; failing here must not stop startup.
            _output?.Dispose();
            _output = null;
            _sink = null;
            _start = _stop = _error = null;
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
    private static byte[] Tone(WaveFormat format, double frequency, int milliseconds, double amplitude = 0.22)
    {
        var rate = format.SampleRate;
        var channels = format.Channels;
        var bytesPerSample = format.BitsPerSample / 8;
        var isFloat = format.Encoding == WaveFormatEncoding.IeeeFloat;

        if (!isFloat && format.BitsPerSample != 16)
        {
            // An exotic shared-mode format. Silence beats noise, and dictation
            // does not depend on the cue.
            return [];
        }

        var samples = rate * milliseconds / 1000;

        // Trailing silence, so playback ends on quiet rather than on the buffer
        // running dry mid-stream. An underrun at the tail is heard as a click or
        // a fragment of the previous tone.
        var tail = rate * 80 / 1000;
        var frameSize = bytesPerSample * channels;
        var buffer = new byte[(samples + tail) * frameSize];
        var fade = Math.Min(samples / 2, rate * 6 / 1000); // 6 ms

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

            var value = Math.Sin(2 * Math.PI * frequency * i / rate) * amplitude * envelope;

            // The same sample into every channel: a mono cue, wherever the
            // device happens to put its speakers.
            for (var channel = 0; channel < channels; channel++)
            {
                var offset = (i * channels + channel) * bytesPerSample;

                if (isFloat)
                {
                    BitConverter.TryWriteBytes(buffer.AsSpan(offset), (float)value);
                }
                else
                {
                    var sample = (short)(value * short.MaxValue);
                    buffer[offset] = (byte)(sample & 0xFF);
                    buffer[offset + 1] = (byte)((sample >> 8) & 0xFF);
                }
            }
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
    private void Play(Func<byte[]?> pick)
    {
        if (!_enabled)
        {
            return;
        }

        _ = Task.Run(() => PlayCore(pick));
    }

    private void PlayCore(Func<byte[]?> pick)
    {
        lock (_gate)
        {
            OpenOutput();

            if (_sink is null)
            {
                return;
            }

            // Picked after the open, because the tones are rendered in the
            // device's format and do not exist until it has one.
            var tone = pick();
            if (tone is null || tone.Length == 0)
            {
                return;
            }

            try
            {
                // Clear first, or queued tones accumulate and play back in
                // sequence, heard as the beep repeating. Never more than one
                // tone pending.
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
        _start = _stop = _error = null;
    }

    internal void Start() => Play(() => _start);

    internal void Stop() => Play(() => _stop);

    internal void Error() => Play(() => _error);

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
