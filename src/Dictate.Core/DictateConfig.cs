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
