using Dictate.Core;
using Xunit;

namespace Dictate.Core.Tests;

public class DiagnosticLogTests
{
    private static (DiagnosticLog Log, StringWriter Sink) Build()
    {
        var sink = new StringWriter();
        return (DiagnosticLog.To(sink), sink);
    }

    [Fact]
    public void An_event_carries_its_name_and_fields()
    {
        var (log, sink) = Build();

        log.Event("utterance", ("n", 3), ("status", UtteranceStatus.Ok));

        var line = sink.ToString();
        Assert.Contains("utterance", line);
        Assert.Contains("n=3", line);
        Assert.Contains("status=Ok", line);
    }

    [Fact]
    public void Timespans_are_reported_in_milliseconds()
    {
        var (log, sink) = Build();

        log.Event("timing", ("transcribe", TimeSpan.FromMilliseconds(1234.7)));

        Assert.Contains("transcribe=1235ms", sink.ToString());
    }

    [Fact]
    public void Newlines_in_a_value_cannot_forge_extra_log_lines()
    {
        // API errors arrive multi-line. Left alone they would break one event
        // into several, and a log that can be spoofed by its own inputs is
        // worse than no log.
        var (log, sink) = Build();

        log.Event("failed", ("error", "line one\nline two\r\nline three"));

        Assert.Single(sink.ToString().TrimEnd().Split('\n'));
    }

    [Fact]
    public void A_very_long_value_is_truncated()
    {
        var (log, sink) = Build();

        log.Event("failed", ("error", new string('x', 5000)));

        Assert.True(sink.ToString().Length < 1000);
        Assert.Contains("…", sink.ToString());
    }

    [Fact]
    public void A_null_value_is_rendered_rather_than_dropped()
    {
        var (log, sink) = Build();

        log.Event("utterance", ("lang", null));

        Assert.Contains("lang=-", sink.ToString());
    }

    [Fact]
    public void The_disabled_log_writes_nothing_and_says_so()
    {
        Assert.False(DiagnosticLog.Disabled.IsEnabled);

        // Must not throw: every call site uses this instance by default.
        DiagnosticLog.Disabled.Event("ignored", ("a", 1));
    }

    [Fact]
    public void Each_event_is_one_line()
    {
        var (log, sink) = Build();

        log.Event("one", ("a", 1));
        log.Event("two", ("b", 2));

        var lines = sink.ToString().TrimEnd().Split(Environment.NewLine);
        Assert.Equal(2, lines.Length);
    }
}
