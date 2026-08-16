using System.Runtime.InteropServices;

namespace Dictate.Windows;

/// <summary>
/// Types text with SendInput using Unicode scan codes, so it works in terminals
/// and over RDP without depending on the target's paste chord — and without
/// touching the clipboard.
/// </summary>
internal static class TextInjector
{
    private static readonly int InputSize = Marshal.SizeOf<Interop.INPUT>();

    /// <summary>
    /// Sends <paramref name="text"/> as keystrokes.
    /// </summary>
    /// <param name="chunkSize">
    /// Characters per SendInput call. The whole string in one call is fastest —
    /// SendInput takes an array, so a paragraph costs one syscall rather than one
    /// per character — but some applications drop events that arrive that fast.
    /// </param>
    /// <param name="chunkDelayMs">Pause between chunks, for the apps that need it.</param>
    internal static void Type(string text, int chunkSize = 200, int chunkDelayMs = 0)
    {
        if (text.Length == 0)
        {
            return;
        }

        chunkSize = Math.Max(1, chunkSize);

        for (var offset = 0; offset < text.Length; offset += chunkSize)
        {
            var length = Math.Min(chunkSize, text.Length - offset);
            SendChunk(text.AsSpan(offset, length));

            if (chunkDelayMs > 0 && offset + length < text.Length)
            {
                Thread.Sleep(chunkDelayMs);
            }
        }
    }

    private static void SendChunk(ReadOnlySpan<char> chunk)
    {
        // Two events per UTF-16 code unit: down then up. Iterating code units
        // rather than runes is deliberate — surrogate pairs must be sent as two
        // events, which is exactly what Windows expects.
        var inputs = new Interop.INPUT[chunk.Length * 2];

        for (var i = 0; i < chunk.Length; i++)
        {
            inputs[i * 2] = KeyEvent(chunk[i], up: false);
            inputs[i * 2 + 1] = KeyEvent(chunk[i], up: true);
        }

        var sent = Interop.SendInput((uint)inputs.Length, inputs, InputSize);
        if (sent != inputs.Length)
        {
            // Most often UIPI: the foreground window belongs to an elevated
            // process and refuses input from this one.
            throw new InvalidOperationException(
                $"SendInput delivered {sent} of {inputs.Length} events (Win32 error {Marshal.GetLastWin32Error()}). " +
                "If the focused window is running as administrator, dictate cannot type into it.");
        }
    }

    private static Interop.INPUT KeyEvent(char character, bool up) => new()
    {
        type = Interop.INPUT_KEYBOARD,
        u = new Interop.INPUTUNION
        {
            ki = new Interop.KEYBDINPUT
            {
                wVk = 0,
                wScan = character,
                dwFlags = Interop.KEYEVENTF_UNICODE | (up ? Interop.KEYEVENTF_KEYUP : 0),
                time = 0,
                dwExtraInfo = Interop.InjectionSignature,
            },
        },
    };
}
