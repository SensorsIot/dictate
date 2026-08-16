namespace Dictate.Core;

/// <summary>
/// Last gate before text becomes keystrokes. The rule that matters: a newline
/// synthesised into a console window is the Enter key, and Enter in a shell runs
/// whatever is on the line. Dictation must never be able to do that.
/// </summary>
public static class TextSanitizer
{
    /// <summary>Terminal-ish executables, matched case-insensitively without extension.</summary>
    private static readonly string[] KnownConsoleProcesses =
    [
        "wezterm-gui", "wezterm", "windowsterminal", "openconsole", "conhost",
        "cmd", "powershell", "pwsh", "putty", "kitty", "alacritty", "mintty",
    ];

    public static bool LooksLikeConsole(string processName, IEnumerable<string>? extra = null)
    {
        if (processName.Length == 0)
        {
            return false;
        }

        var name = Path.GetFileNameWithoutExtension(processName);

        if (KnownConsoleProcesses.Contains(name, StringComparer.OrdinalIgnoreCase))
        {
            return true;
        }

        return extra is not null && extra.Contains(name, StringComparer.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Adds the separator between consecutive dictations, so "…run together."
    /// and "Eins, zwei, drei" do not arrive as one word.
    ///
    /// Applied to consoles too. What is dangerous in a shell is a newline, which
    /// presses Enter and runs the line; a trailing space does nothing at all —
    /// and a terminal is where dictation is used most, so excluding it removed
    /// the separator from exactly the case that needed it.
    /// </summary>
    public static string WithTrailingSpace(string text) =>
        text.Length == 0 ? text : text + " ";

    /// <summary>
    /// Makes <paramref name="text"/> safe to type into <paramref name="target"/>.
    /// In a console every line break collapses to a single space; elsewhere line
    /// breaks are normalised to \r\n and runs of blank lines are capped at one.
    /// </summary>
    public static string ForInjection(string text, TargetContext target)
    {
        if (text.Length == 0)
        {
            return text;
        }

        var normalised = text.Replace("\r\n", "\n").Replace('\r', '\n');

        if (target.IsConsole)
        {
            var parts = normalised.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            return string.Join(' ', parts).Trim();
        }

        var lines = normalised.Split('\n');
        var builder = new List<string>(lines.Length);
        var blankRun = 0;

        foreach (var line in lines)
        {
            if (line.Trim().Length == 0)
            {
                if (++blankRun > 1)
                {
                    continue;
                }
            }
            else
            {
                blankRun = 0;
            }

            builder.Add(line.TrimEnd());
        }

        return string.Join("\r\n", builder).Trim();
    }
}
