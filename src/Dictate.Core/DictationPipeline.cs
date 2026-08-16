using System.Diagnostics;

namespace Dictate.Core;

/// <summary>
/// PCM in, deliverable text out. The whole platform-free half of dictate.
///
/// Degradation policy, in order of preference:
///   Scribe fails  → nothing to say; report the error.
///   Cleanup fails → deliver the raw transcript. A scruffy sentence beats a lost one.
/// </summary>
public sealed class DictationPipeline
{
    private readonly ITranscriber _transcriber;
    private readonly ICleaner _cleaner;
    private readonly DictateConfig _config;

    public DictationPipeline(ITranscriber transcriber, ICleaner cleaner, DictateConfig config)
    {
        _transcriber = transcriber;
        _cleaner = cleaner;
        _config = config;
    }

    public async Task<Utterance> ProcessAsync(byte[] pcm, TargetContext target, CancellationToken ct = default)
    {
        if (pcm.Length == 0)
        {
            return Utterance.Fail("No audio was captured.");
        }

        var wav = Wav.FromPcm16(pcm);

        Transcript transcript;
        var clock = Stopwatch.StartNew();
        try
        {
            transcript = await _transcriber.TranscribeAsync(wav, _config.Language, ct);
        }
        catch (OperationCanceledException) when (ct.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            return Utterance.Fail($"Transcription failed: {ex.Message}");
        }

        var transcribeTime = clock.Elapsed;

        if (transcript.Text.Length == 0)
        {
            return new Utterance
            {
                Status = UtteranceStatus.Failed,
                Text = "",
                LanguageCode = transcript.LanguageCode,
                Error = "Nothing was recognised in the audio.",
                TranscribeTime = transcribeTime,
            };
        }

        clock.Restart();
        string cleaned;
        string? cleanupError = null;
        try
        {
            cleaned = await _cleaner.CleanAsync(transcript.Text, target, ct);

            // An empty cleanup result means the model dropped the utterance —
            // treat that as a failure of cleanup, not as a silent deletion.
            if (cleaned.Length == 0)
            {
                cleanupError = "Cleanup returned nothing.";
                cleaned = transcript.Text;
            }
        }
        catch (OperationCanceledException) when (ct.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            cleanupError = $"Cleanup failed: {ex.Message}";
            cleaned = transcript.Text;
        }

        var cleanupTime = clock.Elapsed;

        return new Utterance
        {
            Status = cleanupError is null ? UtteranceStatus.Ok : UtteranceStatus.CleanupFailed,
            Text = TextSanitizer.ForInjection(cleaned, target),
            RawText = transcript.Text,
            LanguageCode = transcript.LanguageCode,
            Error = cleanupError,
            TranscribeTime = transcribeTime,
            CleanupTime = cleanupTime,
        };
    }
}
