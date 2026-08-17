using System.Diagnostics;
using Dictate.Core;
using NAudio.Wave;

namespace Dictate.Windows;

/// <summary>
/// Records the default input device straight to the format Scribe wants:
/// 16 kHz, mono, 16-bit PCM.
///
/// WaveInEvent (winmm) rather than WasapiCapture: winmm converts to the
/// requested format for us, whereas WASAPI shared mode hands back the device's
/// own mix format — typically 48 kHz stereo float — and would need a resampler
/// in the hot path for no benefit at this quality level.
///
/// Two modes, because opening a capture device costs real time:
///   * default — open on press, close on release. The Windows "microphone in
///     use" indicator is only lit while actually recording, at the cost of the
///     device-open latency on every utterance.
///   * KeepMicrophoneOpen — the device stays open and we simply gate whether
///     incoming buffers are kept. Press-to-recording becomes immediate, but
///     Windows reports the microphone as in use for as long as dictate runs.
/// That trade is the user's to make, so it is configuration rather than a
/// decision baked in here.
/// </summary>
internal sealed class AudioRecorder : IDisposable
{
    /// <summary>
    /// How long a teardown waits for the device to confirm it has stopped before
    /// disposing anyway. Generous — the cost of overshooting is a background
    /// thread parked briefly, the cost of undershooting is the bug this exists
    /// to prevent.
    /// </summary>
    private const int TearDownTimeoutMs = 500;

    private readonly int _maxBytes;
    private readonly bool _keepOpen;
    private readonly object _gate = new();

    private WaveInEvent? _device;
    private MemoryStream? _buffer;
    private bool _capped;
    private bool _disposed;
    private bool _firstBufferSeen;
    private long _startedTicks;

    /// <summary>Raised when <see cref="DictateConfig.MaximumRecordingSeconds"/> is hit.</summary>
    public event Action? LimitReached;

    /// <summary>
    /// Raised after a capture device has been torn down. The flag is false when
    /// the device never confirmed it had stopped, which is the signature of a
    /// handle that did not release — i.e. a microphone Windows still considers
    /// in use. Diagnostics only; nothing branches on it.
    /// </summary>
    public event Action<bool>? DeviceClosed;

    /// <summary>
    /// Raised with how long after <see cref="Start"/> the first audio buffer
    /// actually arrived.
    ///
    /// This separates two failures that look identical from the outside and have
    /// opposite fixes: a capture stream that was stalled by something else in the
    /// process, and an interface that needs a moment to spin up and returns
    /// nothing until it has. Without it, both read as "the microphone produced
    /// only silence".
    /// </summary>
    public event Action<TimeSpan>? FirstBuffer;

    internal AudioRecorder(int maximumSeconds, bool keepMicrophoneOpen)
    {
        _maxBytes = maximumSeconds * Wav.SampleRate * Wav.Channels * Wav.BitsPerSample / 8;
        _keepOpen = keepMicrophoneOpen;
    }

    internal bool IsRecording { get; private set; }

    internal static bool HasInputDevice => WaveInEvent.DeviceCount > 0;

    /// <summary>
    /// Pays the one-time costs — loading winmm, enumerating devices, and in
    /// keep-open mode opening the device — at startup rather than on the first
    /// press, where the user is waiting.
    /// </summary>
    internal void PreWarm()
    {
        if (!HasInputDevice)
        {
            return;
        }

        // Off the caller's thread for two reasons. NFR-19.5: this opens a
        // capture device, and the caller is the UI thread. And the teardown
        // below has to wait for the device to confirm it stopped, which is only
        // possible where NAudio can deliver that callback — not on a thread that
        // is blocking inside the wait.
        _ = Task.Run(() =>
        {
            lock (_gate)
            {
                if (_keepOpen)
                {
                    EnsureDeviceOpen();
                    return;
                }

                // Open and close again, so the first real press does not pay
                // driver initialisation.
                try
                {
                    var warm = new WaveInEvent
                    {
                        WaveFormat = new WaveFormat(Wav.SampleRate, Wav.BitsPerSample, Wav.Channels),
                        BufferMilliseconds = 50,
                    };

                    warm.StartRecording();

                    // Through TearDown rather than Dispose: StopRecording is
                    // asynchronous, and disposing on the next line frees the
                    // buffers while winmm may still own them. That is an access
                    // violation inside waveInUnprepareHeader, not a managed
                    // exception — it takes the process down with no chance to
                    // report anything.
                    TearDown(warm);
                }
                catch (Exception)
                {
                    // Pre-warming is an optimisation. If the device refuses now it
                    // will report the real error on the first press, where there is
                    // a user to tell.
                }
            }
        });
    }

    private void EnsureDeviceOpen()
    {
        if (_device is not null)
        {
            return;
        }

        _device = new WaveInEvent
        {
            WaveFormat = new WaveFormat(Wav.SampleRate, Wav.BitsPerSample, Wav.Channels),
            // 50 ms buffers: small enough that release-to-upload is not waiting
            // on a half-full buffer, large enough to be cheap.
            BufferMilliseconds = 50,
            NumberOfBuffers = 3,
        };

        _device.DataAvailable += OnDataAvailable;
        _device.StartRecording();
    }

    internal void Start()
    {
        lock (_gate)
        {
            if (IsRecording || _disposed)
            {
                return;
            }

            _buffer = new MemoryStream();
            _capped = false;
            _firstBufferSeen = false;

            EnsureDeviceOpen();
            IsRecording = true;
            _startedTicks = Stopwatch.GetTimestamp();
        }
    }

    private void OnDataAvailable(object? sender, WaveInEventArgs e)
    {
        lock (_gate)
        {
            // In keep-open mode this fires continuously; buffers arriving
            // outside a session are dropped here and never stored anywhere.
            if (!IsRecording || _buffer is null || _capped)
            {
                return;
            }

            if (!_firstBufferSeen)
            {
                _firstBufferSeen = true;
                FirstBuffer?.Invoke(Stopwatch.GetElapsedTime(_startedTicks));
            }

            var remaining = _maxBytes - (int)_buffer.Length;
            if (remaining <= 0)
            {
                _capped = true;
                LimitReached?.Invoke();
                return;
            }

            _buffer.Write(e.Buffer, 0, Math.Min(e.BytesRecorded, remaining));
        }
    }

    /// <summary>Stops recording and returns the captured PCM.</summary>
    internal byte[] Stop()
    {
        lock (_gate)
        {
            if (!IsRecording)
            {
                return [];
            }

            IsRecording = false;

            if (!_keepOpen)
            {
                CloseDevice();
            }

            var pcm = _buffer?.ToArray() ?? [];
            _buffer?.Dispose();
            _buffer = null;
            return pcm;
        }
    }

    /// <summary>
    /// Detaches the current device and tears it down on a background thread.
    ///
    /// The handoff is the point. <c>StopRecording</c> is asynchronous: it asks the
    /// callback thread to wind down and returns immediately, so disposing on the
    /// next line races it, and NAudio discards the result of the underlying
    /// waveInClose. When that close loses the race the handle is never released —
    /// silently, permanently, and visibly to the user as a microphone indicator
    /// that stays lit until the process exits.
    ///
    /// Waiting for <c>RecordingStopped</c> is the fix, but it cannot be waited on
    /// here: this runs under <c>_gate</c> on the UI thread, and NAudio raises that
    /// event through the captured SynchronizationContext — which is this thread.
    /// Blocking would wait for a callback that cannot run until we stop blocking.
    /// So the device is detached immediately and drained elsewhere; a new press
    /// opens a fresh one without waiting, at the cost of a brief overlap.
    /// </summary>
    private void CloseDevice()
    {
        var device = _device;
        if (device is null)
        {
            return;
        }

        _device = null;
        device.DataAvailable -= OnDataAvailable;

        _ = Task.Run(() => TearDown(device));
    }

    private void TearDown(WaveInEvent device)
    {
        using var stopped = new ManualResetEventSlim(false);
        void OnStopped(object? sender, StoppedEventArgs e) => stopped.Set();

        device.RecordingStopped += OnStopped;

        var confirmed = false;
        try
        {
            device.StopRecording();
            confirmed = stopped.Wait(TearDownTimeoutMs);
        }
        catch (Exception)
        {
            // A device unplugged mid-utterance throws here; whatever was
            // captured before that is still worth sending.
        }
        finally
        {
            device.RecordingStopped -= OnStopped;

            try
            {
                device.Dispose();
            }
            catch (Exception)
            {
                // Nothing useful left to do — the handle is the driver's problem now.
            }
        }

        DeviceClosed?.Invoke(confirmed);
    }

    public void Dispose()
    {
        WaveInEvent? device;

        lock (_gate)
        {
            _disposed = true;
            IsRecording = false;

            device = _device;
            _device = null;
            _buffer?.Dispose();
            _buffer = null;
        }

        // Torn down inline rather than handed off: this is process shutdown, and
        // a background task racing the exit is how the handle leaks anyway.
        if (device is not null)
        {
            device.DataAvailable -= OnDataAvailable;
            TearDown(device);
        }
    }
}
