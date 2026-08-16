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
/// </summary>
internal sealed class AudioRecorder : IDisposable
{
    private readonly int _maxBytes;
    private readonly object _gate = new();

    private WaveInEvent? _device;
    private MemoryStream? _buffer;
    private bool _capped;

    /// <summary>Raised when <see cref="DictateConfig.MaximumRecordingSeconds"/> is hit.</summary>
    public event Action? LimitReached;

    internal AudioRecorder(int maximumSeconds)
    {
        _maxBytes = maximumSeconds * Wav.SampleRate * Wav.Channels * Wav.BitsPerSample / 8;
    }

    internal bool IsRecording { get; private set; }

    internal static bool HasInputDevice => WaveInEvent.DeviceCount > 0;

    internal void Start()
    {
        lock (_gate)
        {
            if (IsRecording)
            {
                return;
            }

            _buffer = new MemoryStream();
            _capped = false;

            _device = new WaveInEvent
            {
                WaveFormat = new WaveFormat(Wav.SampleRate, Wav.BitsPerSample, Wav.Channels),
                // 50 ms buffers: small enough that release-to-upload is not
                // waiting on a half-full buffer, large enough to be cheap.
                BufferMilliseconds = 50,
                NumberOfBuffers = 3,
            };

            _device.DataAvailable += OnDataAvailable;
            _device.StartRecording();
            IsRecording = true;
        }
    }

    private void OnDataAvailable(object? sender, WaveInEventArgs e)
    {
        lock (_gate)
        {
            if (_buffer is null || _capped)
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

            try
            {
                _device?.StopRecording();
            }
            catch (Exception)
            {
                // A device unplugged mid-utterance throws here; whatever was
                // captured before that is still worth sending.
            }

            if (_device is not null)
            {
                _device.DataAvailable -= OnDataAvailable;
                _device.Dispose();
                _device = null;
            }

            var pcm = _buffer?.ToArray() ?? [];
            _buffer?.Dispose();
            _buffer = null;
            return pcm;
        }
    }

    public void Dispose() => Stop();
}
