using System.Buffers.Binary;
using System.Text;
using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class WavTests
{
    [Fact]
    public void Header_describes_16k_mono_16_bit()
    {
        var pcm = new byte[1000];

        var wav = Wav.FromPcm16(pcm);

        Assert.Equal(44 + pcm.Length, wav.Length);
        Assert.Equal("RIFF", Encoding.ASCII.GetString(wav, 0, 4));
        Assert.Equal("WAVE", Encoding.ASCII.GetString(wav, 8, 4));
        Assert.Equal("fmt ", Encoding.ASCII.GetString(wav, 12, 4));
        Assert.Equal("data", Encoding.ASCII.GetString(wav, 36, 4));

        Assert.Equal(1, BinaryPrimitives.ReadUInt16LittleEndian(wav.AsSpan(20, 2)));       // PCM
        Assert.Equal(1, BinaryPrimitives.ReadUInt16LittleEndian(wav.AsSpan(22, 2)));       // mono
        Assert.Equal(16_000u, BinaryPrimitives.ReadUInt32LittleEndian(wav.AsSpan(24, 4))); // sample rate
        Assert.Equal(32_000u, BinaryPrimitives.ReadUInt32LittleEndian(wav.AsSpan(28, 4))); // byte rate
        Assert.Equal(2, BinaryPrimitives.ReadUInt16LittleEndian(wav.AsSpan(32, 2)));       // block align
        Assert.Equal(16, BinaryPrimitives.ReadUInt16LittleEndian(wav.AsSpan(34, 2)));      // bit depth
    }

    [Fact]
    public void Declared_sizes_match_the_payload()
    {
        var wav = Wav.FromPcm16(new byte[1000]);

        Assert.Equal(36u + 1000u, BinaryPrimitives.ReadUInt32LittleEndian(wav.AsSpan(4, 4)));
        Assert.Equal(1000u, BinaryPrimitives.ReadUInt32LittleEndian(wav.AsSpan(40, 4)));
    }

    [Fact]
    public void Audio_is_copied_verbatim()
    {
        var pcm = new byte[] { 1, 2, 3, 4, 250, 251 };

        var wav = Wav.FromPcm16(pcm);

        Assert.Equal(pcm, wav[44..]);
    }

    [Fact]
    public void Duration_is_one_second_for_one_seconds_worth_of_bytes()
    {
        Assert.Equal(1.0, Wav.DurationOf(32_000).TotalSeconds, 3);
    }
}

public class ScribeParseTests
{
    [Fact]
    public void Text_and_language_are_extracted()
    {
        var transcript = ScribeTranscriber.Parse(
            """{"language_code":"de","language_probability":0.98,"text":"Guten Morgen","words":[]}""");

        Assert.Equal("Guten Morgen", transcript.Text);
        Assert.Equal("de", transcript.LanguageCode);
    }

    [Fact]
    public void Surrounding_whitespace_is_trimmed()
    {
        var transcript = ScribeTranscriber.Parse("""{"text":"  hello  "}""");

        Assert.Equal("hello", transcript.Text);
    }

    [Fact]
    public void A_missing_language_is_not_fatal()
    {
        var transcript = ScribeTranscriber.Parse("""{"text":"hello"}""");

        Assert.Null(transcript.LanguageCode);
    }

    [Fact]
    public void A_response_without_text_is_an_error()
    {
        Assert.Throws<TranscriptionException>(() => ScribeTranscriber.Parse("""{"detail":"nope"}"""));
    }
}

public class CleanupPromptTests
{
    [Fact]
    public void The_system_prompt_forbids_translating_and_answering()
    {
        var prompt = CleanupPrompt.BuildSystem(new DictateConfig());

        Assert.Contains("Translate", prompt, StringComparison.OrdinalIgnoreCase);
        Assert.Contains("Answer", prompt, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Vocabulary_is_listed_for_the_model()
    {
        var config = new DictateConfig { Vocabulary = { "HB9XYZ" } };

        Assert.Contains("HB9XYZ", CleanupPrompt.BuildSystem(config));
    }

    [Fact]
    public void An_empty_vocabulary_does_not_produce_a_dangling_heading()
    {
        var config = new DictateConfig { Vocabulary = new List<string>() };

        Assert.DoesNotContain("Spell these terms", CleanupPrompt.BuildSystem(config));
    }

    [Fact]
    public void Extra_instructions_are_appended()
    {
        var config = new DictateConfig { ExtraCleanupInstruction = "Prefer British spelling." };

        Assert.Contains("Prefer British spelling.", CleanupPrompt.BuildSystem(config));
    }

    [Fact]
    public void The_user_prompt_carries_the_transcript_and_the_window()
    {
        var prompt = CleanupPrompt.BuildUser("hello there", new TargetContext("Code", "Program.cs", false));

        Assert.Contains("hello there", prompt);
        Assert.Contains("Code", prompt);
        Assert.Contains("Program.cs", prompt);
    }

    [Fact]
    public void A_console_target_asks_for_a_single_line()
    {
        var prompt = CleanupPrompt.BuildUser("git status", new TargetContext("wezterm-gui", "dev-1", true));

        Assert.Contains("single line", prompt);
    }
}
