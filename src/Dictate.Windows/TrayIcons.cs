using System.Drawing.Drawing2D;

namespace Dictate.Windows;

/// <summary>
/// Draws the tray icon at runtime, one per state, so the repository carries no
/// binary .ico files and the colours live next to the states they mean.
/// </summary>
internal sealed class TrayIcons : IDisposable
{
    private readonly Dictionary<SessionState, Icon> _icons = new();
    private readonly List<IntPtr> _handles = [];

    internal TrayIcons()
    {
        _icons[SessionState.Idle] = Build(Color.FromArgb(148, 163, 184));        // slate
        _icons[SessionState.Recording] = Build(Color.FromArgb(239, 68, 68));     // red
        _icons[SessionState.Transcribing] = Build(Color.FromArgb(59, 130, 246)); // blue
    }

    internal Icon For(SessionState state) => _icons[state];

    private Icon Build(Color colour)
    {
        using var bitmap = new Bitmap(32, 32);
        using (var graphics = Graphics.FromImage(bitmap))
        {
            graphics.SmoothingMode = SmoothingMode.AntiAlias;
            graphics.Clear(Color.Transparent);

            using var brush = new SolidBrush(colour);
            graphics.FillEllipse(brush, 4, 4, 24, 24);

            using var pen = new Pen(Color.FromArgb(200, 255, 255, 255), 2f);
            graphics.DrawEllipse(pen, 4, 4, 24, 24);
        }

        var handle = bitmap.GetHicon();
        _handles.Add(handle);

        // Clone so the Icon owns managed memory we control; the raw handle is
        // released in Dispose.
        using var temporary = Icon.FromHandle(handle);
        return (Icon)temporary.Clone();
    }

    public void Dispose()
    {
        foreach (var icon in _icons.Values)
        {
            icon.Dispose();
        }

        foreach (var handle in _handles)
        {
            Interop.DestroyIcon(handle);
        }

        _icons.Clear();
        _handles.Clear();
    }
}
