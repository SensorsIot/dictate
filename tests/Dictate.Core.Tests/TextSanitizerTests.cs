using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class TextSanitizerTests
{
    private static TargetContext Console(string process = "wezterm-gui") => new(process, "", true);
    private static TargetContext Editor => new("Code", "main.cs", false);

    [Fact]
    public void Console_target_collapses_every_newline_to_a_space()
    {
        // The whole point: a newline typed into a shell is Enter, and Enter runs
        // whatever came before it.
        var result = TextSanitizer.ForInjection("git status\nrm -rf /tmp/x", Console());

        Assert.DoesNotContain('\n', result);
        Assert.DoesNotContain('\r', result);
        Assert.Equal("git status rm -rf /tmp/x", result);
    }

    [Theory]
    [InlineData("one\r\ntwo")]
    [InlineData("one\rtwo")]
    [InlineData("one\ntwo")]
    public void Console_target_handles_every_line_ending_flavour(string input)
    {
        Assert.Equal("one two", TextSanitizer.ForInjection(input, Console()));
    }

    [Fact]
    public void Non_console_target_keeps_paragraphs_as_crlf()
    {
        var result = TextSanitizer.ForInjection("First line.\nSecond line.", Editor);

        Assert.Equal("First line.\r\nSecond line.", result);
    }

    [Fact]
    public void Runs_of_blank_lines_are_capped_at_one()
    {
        var result = TextSanitizer.ForInjection("a\n\n\n\nb", Editor);

        Assert.Equal("a\r\n\r\nb", result);
    }

    [Fact]
    public void Surrounding_whitespace_is_trimmed()
    {
        Assert.Equal("hello", TextSanitizer.ForInjection("  \n hello \n  ", Editor));
    }

    [Fact]
    public void Empty_text_stays_empty()
    {
        Assert.Equal("", TextSanitizer.ForInjection("", Editor));
    }

    [Fact]
    public void A_separator_is_added_so_consecutive_dictations_do_not_run_together()
    {
        Assert.Equal("run together. ", TextSanitizer.WithTrailingSpace("run together."));
    }

    [Fact]
    public void The_separator_applies_in_consoles_too()
    {
        // The original bug: the space was suppressed for console targets out of
        // misplaced caution. A newline in a shell presses Enter; a space does
        // nothing — and the terminal is where dictation is used most.
        var text = TextSanitizer.ForInjection("git status", Console());

        Assert.Equal("git status ", TextSanitizer.WithTrailingSpace(text));
    }

    [Fact]
    public void No_separator_is_added_to_nothing()
    {
        Assert.Equal("", TextSanitizer.WithTrailingSpace(""));
    }

    [Fact]
    public void Only_one_separator_is_added_per_utterance()
    {
        Assert.Equal("done. ", TextSanitizer.WithTrailingSpace(TextSanitizer.ForInjection("done.", Editor)));
    }

    [Theory]
    [InlineData("wezterm-gui")]
    [InlineData("WezTerm-GUI")]
    [InlineData("cmd")]
    [InlineData("powershell")]
    [InlineData("WindowsTerminal")]
    public void Known_terminals_are_recognised_regardless_of_case(string process)
    {
        Assert.True(TextSanitizer.LooksLikeConsole(process));
    }

    [Theory]
    [InlineData("Code")]
    [InlineData("olk")]
    [InlineData("")]
    public void Ordinary_apps_are_not_consoles(string process)
    {
        Assert.False(TextSanitizer.LooksLikeConsole(process));
    }

    [Fact]
    public void Extension_is_ignored_when_matching()
    {
        Assert.True(TextSanitizer.LooksLikeConsole("cmd.exe"));
    }

    [Fact]
    public void User_supplied_console_processes_are_honoured()
    {
        Assert.True(TextSanitizer.LooksLikeConsole("myshell", new[] { "myshell" }));
        Assert.False(TextSanitizer.LooksLikeConsole("myshell"));
    }
}
