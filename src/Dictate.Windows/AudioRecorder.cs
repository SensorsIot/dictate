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
    private readonly int _maxBytes;
    private readonly bool _keepOpen;
    private readonly object _gate = new();

    private WaveInEvent? _device;
    private MemoryStream? _buffer;
    private bool _capped;
    private bool _disposed;

    /// <summary>Raised when <see cref="DictateConfig.MaximumRecordingSeconds"/> is hit.</summary>
    public event Action? LimitReached;

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

        lock (_gate)
        {
            if (_keepOpen)
            {
                EnsureDeviceOpen();
                return;
            }

            // Open and immediately close, so the first real press does not pay
            // driver initialisation.
            try
            {
                using var warm = new WaveInEvent
                {
                    WaveFormat = new WaveFormat(Wav.SampleRate, Wav.BitsPerSample, Wav.Channels),
                    BufferMilliseconds = 50,
                };
                warm.StartRecording();
                warm.StopRecording();
            }
            catch (Exception)
            {
                // Pre-warming is an optimisation. If the device refuses now it
                // will report the real error on the first press, where there is
                // a user to tell.
            }
        }
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

            EnsureDeviceOpen();
            IsRecording = true;
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

    private void CloseDevice()
    {
        if (_device is null)
        {
            return;
        }

        try
        {
            _device.StopRecording();
        }
        catch (Exception)
        {
            // A device unplugged mid-utterance throws here; whatever was
            // captured before that is still worth sending.
        }

        _device.DataAvailable -= OnDataAvailable;
        _device.Dispose();
        _device = null;
    }

    public void Dispose()
    {
        lock (_gate)
        {
            _disposed = true;
            IsRecording = false;
            CloseDevice();
            _buffer?.Dispose();
            _buffer = null;
        }
    }
}
