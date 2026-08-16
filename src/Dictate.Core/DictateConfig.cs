using System.Text.Json;
using System.Text.Json.Serialization;

namespace Dictate.Core;

/// <summary>
/// Everything the user can change, loaded from
/// <c>%APPDATA%\dictate\config.json</c>. Missing file means defaults; a broken
/// file is a hard error, because silently falling back to defaults would look
/// like the settings simply did not take.
/// </summary>
public sealed class DictateConfig
{
    /// <summary>
    /// Virtual-key code held to dictate. Default 0xA3 = VK_RCONTROL.
    /// Right Alt is deliberately not the default: on a Swiss layout it is AltGr,
    /// and swallowing it would cost the user @ { } [ ] \.
    /// </summary>
    public int HotkeyVirtualKey { get; set; } = 0xA3;

    /// <summary>
    /// Whether the hotkey is swallowed instead of reaching the focused window.
    /// Off by default (FR-8.4): Right Ctrl alone does nothing in almost every
    /// application, whereas swallowing it breaks every Ctrl+key combination made
    /// with that hand.
    /// </summary>
    public bool SuppressHotkey { get; set; }

    /// <summary>Presses shorter than this are treated as an accidental tap and ignored.</summary>
    public int MinimumHoldMs { get; set; } = 300;

    /// <summary>Recording stops here regardless, so a stuck key cannot upload the whole afternoon.</summary>
    public int MaximumRecordingSeconds { get; set; } = 120;

    /// <summary>
    /// Peak amplitude (0–32767) below which a recording counts as silence and is
    /// not uploaded. 200 is roughly -44 dBFS.
    /// </summary>
    public int SilenceThreshold { get; set; } = 200;

    /// <summary>
    /// Keep the capture device open for the lifetime of the process instead of
    /// opening it on each press.
    ///
    /// Off by default because Windows then reports the microphone as in use the
    /// whole time dictate runs. Worth turning on for USB interfaces that need a
    /// moment to spin their capture stream up — a cold open can return valid
    /// frames of digital silence, which looks exactly like the application being
    /// broken on the first dictation after a boot or after the interface idles.
    /// </summary>
    public bool KeepMicrophoneOpen { get; set; }

    /// <summary>
    /// Type a space after each utterance, so consecutive dictations do not run
    /// into each other.
    /// </summary>
    public bool AppendSpaceAfterInsert { get; set; } = true;

    /// <summary>
    /// Put every delivered utterance on the clipboard as well as typing it, so
    /// a sentence that landed in the wrong window can be pasted into the right
    /// one without going through the tray menu.
    ///
    /// The cost is that dictating overwrites whatever you had copied. That is a
    /// real trade — turn it off if you routinely dictate while holding
    /// something in the clipboard.
    /// </summary>
    public bool AlwaysCopyToClipboard { get; set; } = true;

    /// <summary>Report press-to-recording, transcription and cleanup timings after each utterance.</summary>
    public bool ShowTimings { get; set; }

    /// <summary>
    /// Write a diagnostic log to <c>%LOCALAPPDATA%\dictate\dictate.log</c>:
    /// lifecycle, timings, outcomes and process counters. Never the dictated
    /// text — see <see cref="DiagnosticLog"/>.
    ///
    /// Off by default, because D-12 chose zero persistence. Turn it on when
    /// something needs diagnosing from another machine.
    /// </summary>
    public bool EnableDiagnosticLog { get; set; }

    public LanguageMode Language { get; set; } = LanguageMode.Auto;

    /// <summary>ElevenLabs transcription model. Configurable because the Scribe generation moves.</summary>
    public string ScribeModelId { get; set; } = "scribe_v2";

    /// <summary>Cleanup model. Haiku 4.5 is the intended tier — cheap and fast enough to sit in the loop.</summary>
    public string CleanupModel { get; set; } = "claude-haiku-4-5";

    public int ScribeTimeoutSeconds { get; set; } = 15;
    public int CleanupTimeoutSeconds { get; set; } = 10;

    /// <summary>
    /// Terms Scribe reliably gets wrong and cleanup should correct. Project names,
    /// call signs, jargon. Keep it short — it rides in every request.
    /// </summary>
    public List<string> Vocabulary { get; set; } = new()
    {
        "AREDN", "ESP-IDF", "ESP32", "MQTT", "Infisical", "Modbus",
        "SUN2000", "Nulleinspeisung", "devcontainer", "WezTerm",
    };

    /// <summary>
    /// Extra instruction appended to the cleanup prompt. An escape hatch for
    /// habits the generic prompt does not cover.
    /// </summary>
    public string? ExtraCleanupInstruction { get; set; }

    /// <summary>
    /// Process names (no extension, case-insensitive) treated as consoles, in
    /// addition to the built-in list. Newlines are never typed into these.
    /// </summary>
    public List<string> ExtraConsoleProcesses { get; set; } = new();

    /// <summary>Characters per SendInput batch. Lower it for apps that drop fast input.</summary>
    public int InjectionChunkSize { get; set; } = 200;

    /// <summary>Milliseconds between injection batches.</summary>
    public int InjectionChunkDelayMs { get; set; }

    public bool PlaySounds { get; set; } = true;
    public bool ShowOverlay { get; set; } = true;

    /// <summary>
    /// Where the recording indicator appears. Lower right of the main screen by
    /// default: a fixed corner is somewhere you can learn to glance at, whereas
    /// one that follows the cursor is never twice in the same place.
    /// </summary>
    public OverlayPosition OverlayPosition { get; set; } = OverlayPosition.BottomRight;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true,
        Converters = { new JsonStringEnumConverter() },
        ReadCommentHandling = JsonCommentHandling.Skip,
        AllowTrailingCommas = true,
    };

    public static DictateConfig Load(string path)
    {
        if (!File.Exists(path))
        {
            return new DictateConfig();
        }

        var json = File.ReadAllText(path);
        return JsonSerializer.Deserialize<DictateConfig>(json, JsonOptions)
               ?? throw new InvalidDataException($"{path} parsed to null.");
    }

    public void Save(string path)
    {
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllText(path, JsonSerializer.Serialize(this, JsonOptions));
    }
}
