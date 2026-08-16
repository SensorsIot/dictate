using System.Globalization;
using System.Text;

namespace Dictate.Core;

/// <summary>
/// An opt-in diagnostic log: lifecycle events, timings and outcomes.
///
/// **It never records what was dictated.** FR-17.1 says no transcript reaches
/// disk, and that holds here — the API takes a short event name plus named
/// fields, so there is no path by which utterance text can be passed in by
/// accident. Where the size of an utterance matters, log its length.
///
/// Off by default: the user chose zero persistence. It exists because diagnosing a
/// shipped binary from another machine otherwise means reconstructing its
/// behaviour from Windows event logs, which is slow and often inconclusive —
/// a silent exit and a deliberate quit look identical from outside.
/// </summary>
public sealed class DiagnosticLog : IDisposable
{
    private readonly TextWriter? _writer;
    private readonly object _gate = new();

    /// <summary>A log that discards everything. The default.</summary>
    public static readonly DiagnosticLog Disabled = new((TextWriter?)null);

    private DiagnosticLog(TextWriter? writer) => _writer = writer;

    public bool IsEnabled => _writer is not null;

    /// <summary>For tests: writes to the supplied writer.</summary>
    public static DiagnosticLog To(TextWriter writer) => new(writer);

    /// <summary>
    /// Opens <paramref name="path"/> for appending, truncating it first if it
    /// has grown past <paramref name="maxBytes"/>. A diagnostic log that fills a
    /// disk is a worse bug than the one it was added to find.
    /// </summary>
    public static DiagnosticLog OpenFile(string path, long maxBytes = 2 * 1024 * 1024)
    {
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        if (File.Exists(path) && new FileInfo(path).Length > maxBytes)
        {
            File.Delete(path);
        }

        var stream = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.ReadWrite);
        var writer = new StreamWriter(stream, new UTF8Encoding(false)) { AutoFlush = true };
        return new DiagnosticLog(writer);
    }

    /// <summary>
    /// Writes one event. Field values are formatted invariantly; newlines are
    /// stripped so a multi-line API error cannot forge extra log lines.
    /// </summary>
    public void Event(string name, params (string Key, object? Value)[] fields)
    {
        if (_writer is null)
        {
            return;
        }

        var line = new StringBuilder()
            .Append(DateTimeOffset.Now.ToString("yyyy-MM-dd HH:mm:ss.fff", CultureInfo.InvariantCulture))
            .Append("  ")
            .Append(name);

        foreach (var (key, value) in fields)
        {
            line.Append("  ").Append(key).Append('=').Append(Format(value));
        }

        lock (_gate)
        {
            _writer.WriteLine(line.ToString());
        }
    }

    private static string Format(object? value) => value switch
    {
        null => "-",
        TimeSpan span => $"{span.TotalMilliseconds:0}ms",
        double number => number.ToString("0.###", CultureInfo.InvariantCulture),
        IFormattable formattable => formattable.ToString(null, CultureInfo.InvariantCulture),
        _ => Sanitise(value.ToString()),
    };

    private static string Sanitise(string? text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return "-";
        }

        var flattened = text.Replace("\r", " ").Replace("\n", " ").Trim();
        return flattened.Length > 300 ? flattened[..300] + "…" : flattened;
    }

    public void Dispose()
    {
        lock (_gate)
        {
            _writer?.Flush();
            _writer?.Dispose();
        }
    }
}
