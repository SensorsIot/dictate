using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class PipelineTests
{
    /// <summary>0.1 s of audio with a real signal in it, so the silence guard passes.</summary>
    private static readonly byte[] SomeAudio = Speech(3200);

    private static byte[] Speech(int bytes)
    {
        var buffer = new byte[bytes];
        for (var i = 0; i + 1 < bytes; i += 2)
        {
            // Alternating ±8000: crude, but well above the silence threshold.
            var sample = (short)(i % 4 == 0 ? 8000 : -8000);
            buffer[i] = (byte)(sample & 0xFF);
            buffer[i + 1] = (byte)((sample >> 8) & 0xFF);
        }

        return buffer;
    }

    private sealed class StubTranscriber(Transcript? result = null, Exception? failure = null) : ITranscriber
    {
        public LanguageMode SeenLanguage { get; private set; }

        public Task<Transcript> TranscribeAsync(byte[] wav, LanguageMode language, CancellationToken ct)
        {
            SeenLanguage = language;
            return failure is not null
                ? Task.FromException<Transcript>(failure)
                : Task.FromResult(result ?? new Transcript("raw text", "en"));
        }
    }

    private sealed class StubCleaner(string? result = null, Exception? failure = null) : ICleaner
    {
        public TargetContext? SeenTarget { get; private set; }

        public Task<string> CleanAsync(string transcript, TargetContext target, CancellationToken ct)
        {
            SeenTarget = target;
            return failure is not null
                ? Task.FromException<string>(failure)
                : Task.FromResult(result ?? "cleaned text");
        }
    }

    private static DictationPipeline Build(ITranscriber t, ICleaner c, DictateConfig? config = null) =>
        new(t, c, config ?? new DictateConfig());

    [Fact]
    public async Task Happy_path_delivers_the_cleaned_text()
    {
        var pipeline = Build(new StubTranscriber(new Transcript("um so the thing is", "en")),
                             new StubCleaner("So the thing is."));

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Ok, result.Status);
        Assert.Equal("So the thing is.", result.Text);
        Assert.Equal("um so the thing is", result.RawText);
        Assert.Null(result.Error);
    }

    [Fact]
    public async Task Cleanup_failure_falls_back_to_the_raw_transcript()
    {
        // The agreed degradation: a scruffy sentence beats a lost one.
        var pipeline = Build(new StubTranscriber(new Transcript("um so the thing is", "en")),
                             new StubCleaner(failure: new HttpRequestException("503")));

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.CleanupFailed, result.Status);
        Assert.Equal("um so the thing is", result.Text);
        Assert.Contains("Cleanup failed", result.Error);
    }

    [Fact]
    public async Task Cleanup_returning_nothing_is_treated_as_a_failure_not_a_deletion()
    {
        var pipeline = Build(new StubTranscriber(new Transcript("keep me", "en")),
                             new StubCleaner(""));

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.CleanupFailed, result.Status);
        Assert.Equal("keep me", result.Text);
    }

    [Fact]
    public async Task Transcription_failure_yields_no_text()
    {
        var pipeline = Build(new StubTranscriber(failure: new TranscriptionException("Scribe returned 401")),
                             new StubCleaner());

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Failed, result.Status);
        Assert.False(result.HasText);
        Assert.Contains("401", result.Error);
    }

    [Fact]
    public async Task Silence_is_reported_rather_than_typed()
    {
        var pipeline = Build(new StubTranscriber(new Transcript("", "en")), new StubCleaner());

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Failed, result.Status);
        Assert.False(result.HasText);
    }

    [Fact]
    public async Task Silent_audio_never_reaches_the_network_and_blames_the_microphone()
    {
        // The failure this exists for: a USB interface that returns well-formed
        // frames of digital silence before its capture stream has spun up.
        // Uploading those costs money and comes back as "nothing recognised",
        // which points at the speech model instead of the input device.
        var transcriber = new StubTranscriber(failure: new Exception("must not be called"));
        var pipeline = Build(transcriber, new StubCleaner());

        var result = await pipeline.ProcessAsync(new byte[32_000], TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Failed, result.Status);
        Assert.Contains("microphone", result.Error, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("recognised", result.Error, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task The_silence_threshold_is_configurable()
    {
        var quiet = new DictateConfig { SilenceThreshold = 1 };
        var pipeline = Build(new StubTranscriber(new Transcript("heard it", "en")),
                             new StubCleaner("Heard it."), quiet);

        var result = await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Ok, result.Status);
    }

    [Fact]
    public async Task Empty_audio_never_reaches_the_network()
    {
        var transcriber = new StubTranscriber(failure: new Exception("must not be called"));
        var pipeline = Build(transcriber, new StubCleaner());

        var result = await pipeline.ProcessAsync([], TargetContext.Unknown);

        Assert.Equal(UtteranceStatus.Failed, result.Status);
        Assert.Contains("No audio", result.Error);
    }

    [Fact]
    public async Task Console_targets_get_single_line_output()
    {
        var pipeline = Build(new StubTranscriber(new Transcript("raw", "en")),
                             new StubCleaner("line one\nline two"));

        var result = await pipeline.ProcessAsync(SomeAudio, new TargetContext("wezterm-gui", "dev-1", true));

        Assert.Equal("line one line two", result.Text);
    }

    [Fact]
    public async Task The_pinned_language_reaches_the_transcriber()
    {
        var transcriber = new StubTranscriber();
        var pipeline = Build(transcriber, new StubCleaner(), new DictateConfig { Language = LanguageMode.German });

        await pipeline.ProcessAsync(SomeAudio, TargetContext.Unknown);

        Assert.Equal(LanguageMode.German, transcriber.SeenLanguage);
    }

    [Fact]
    public async Task The_focused_window_reaches_the_cleaner()
    {
        var cleaner = new StubCleaner();
        var target = new TargetContext("Code", "Program.cs", false);
        var pipeline = Build(new StubTranscriber(), cleaner);

        await pipeline.ProcessAsync(SomeAudio, target);

        Assert.Equal(target, cleaner.SeenTarget);
    }
}
