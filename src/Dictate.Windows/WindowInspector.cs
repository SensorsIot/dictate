using System.Diagnostics;
using Dictate.Core;

namespace Dictate.Windows;

/// <summary>
/// Identifies the window that had focus when the hotkey went down. Captured at
/// press time and re-checked at delivery time: if focus moved in between, the
/// text goes to the clipboard rather than into whatever is in front now.
/// </summary>
internal static class WindowInspector
{
    internal static IntPtr Foreground() => Interop.GetForegroundWindow();

    internal static TargetContext Describe(IntPtr window, IEnumerable<string>? extraConsoleProcesses = null)
    {
        if (window == IntPtr.Zero)
        {
            return TargetContext.Unknown;
        }

        var title = ReadTitle(window);
        var process = ReadProcessName(window);
        var isConsole = TextSanitizer.LooksLikeConsole(process, extraConsoleProcesses)
                        || IsConsoleWindowClass(window);

        return new TargetContext(process, title, isConsole);
    }

    private static string ReadTitle(IntPtr window)
    {
        var buffer = new char[512];
        var length = Interop.GetWindowTextW(window, buffer, buffer.Length);
        return length > 0 ? new string(buffer, 0, length) : "";
    }

    private static string ReadProcessName(IntPtr window)
    {
        if (Interop.GetWindowThreadProcessId(window, out var pid) == 0 || pid == 0)
        {
            return "";
        }

        try
        {
            using var process = Process.GetProcessById((int)pid);
            return process.ProcessName;
        }
        catch (ArgumentException)
        {
            return ""; // exited between the two calls
        }
        catch (InvalidOperationException)
        {
            return "";
        }
    }

    /// <summary>
    /// Catches consoles the process name misses — a console window hosted by
    /// conhost still reports the class ConsoleWindowClass regardless of which
    /// executable is running inside it.
    /// </summary>
    private static bool IsConsoleWindowClass(IntPtr window)
    {
        var buffer = new char[256];
        var length = Interop.GetClassNameW(window, buffer, buffer.Length);
        if (length <= 0)
        {
            return false;
        }

        var className = new string(buffer, 0, length);
        return className is "ConsoleWindowClass" or "PseudoConsoleWindow";
    }
}
