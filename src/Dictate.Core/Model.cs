namespace Dictate.Core;

/// <summary>
/// What had focus when the hotkey went down. Captured on the Windows side and
/// handed to the pipeline so cleanup can adapt formatting, and so the injector
/// knows whether it is typing into a console.
/// </summary>
/// <param name="ProcessName">Executable name without extension, e.g. "wezterm-gui".</param>
/// <param name="WindowTitle">Title bar text, often the best hint at what the user is doing.</param>
/// <param name="IsConsole">True for terminal-like windows, where a newline would press Enter.</param>
public sealed record TargetContext(string ProcessName, string WindowTitle, bool IsConsole)
{
    public static readonly TargetContext Unknown = new("", "", false);
}

/// <summary>Where the recording indicator sits.</summary>
public enum OverlayPosition
{
    /// <summary>Lower right of the main screen, above the taskbar. Predictable — you learn where to glance.</summary>
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,

    /// <summary>Follows the mouse. Close to where you are looking, but never in the same place twice.</summary>
    NearCursor,
}

/// <summary>Which language Scribe should assume for an utterance.</summary>
public enum LanguageMode
{
    /// <summary>Let Scribe decide. Wrong on short utterances often enough to want the override.</summary>
    Auto,
    German,
    English,
}

public static class LanguageModeExtensions
{
    /// <summary>ISO-639-1 code for the Scribe request, or null to let it auto-detect.</summary>
    public static string? ToLanguageCode(this LanguageMode mode) => mode switch
    {
        LanguageMode.German => "de",
        LanguageMode.English => "en",
        _ => null,
    };
}

/// <summary>The raw result of transcription, before cleanup.</summary>
/// <param name="Text">Verbatim transcript, fillers and all.</param>
/// <param name="LanguageCode">What Scribe detected, or the code we pinned.</param>
public sealed record Transcript(string Text, string? LanguageCode);

/// <summary>Why an utterance did not come back clean.</summary>
public enum UtteranceStatus
{
    /// <summary>Transcribed and cleaned.</summary>
    Ok,

    /// <summary>Transcribed, but cleanup failed — <see cref="Utterance.Text"/> is the raw transcript.</summary>
    CleanupFailed,

    /// <summary>Nothing usable. <see cref="Utterance.Error"/> says why.</summary>
    Failed,
}

/// <summary>One press-to-release cycle, start to finish.</summary>
public sealed record Utterance
{
    public required UtteranceStatus Status { get; init; }

    /// <summary>The text to deliver. Empty when <see cref="Status"/> is <see cref="UtteranceStatus.Failed"/>.</summary>
    public required string Text { get; init; }

    /// <summary>The verbatim transcript, kept so the tray can offer it when cleanup went wrong.</summary>
    public string RawText { get; init; } = "";

    public string? LanguageCode { get; init; }
    public string? Error { get; init; }

    public TimeSpan TranscribeTime { get; init; }
    public TimeSpan CleanupTime { get; init; }

    public bool HasText => Text.Length > 0;

    public static Utterance Fail(string error) =>
        new() { Status = UtteranceStatus.Failed, Text = "", Error = error };
}
