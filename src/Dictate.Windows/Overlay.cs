using Dictate.Core;

namespace Dictate.Windows;

internal enum SessionState
{
    Idle,
    Recording,
    Transcribing,
}

/// <summary>
/// A small always-on-top pill near the cursor showing what dictate is doing.
///
/// The one non-negotiable property is that it never takes focus (FR-14.4): the
/// pinned-target contract records the foreground window at hotkey-down, and an
/// overlay that activated itself would break that on every single utterance.
/// WS_EX_NOACTIVATE plus ShowWithoutActivation is what guarantees it.
/// </summary>
internal sealed class Overlay : Form
{
    private const int WS_EX_NOACTIVATE = 0x08000000;
    private const int WS_EX_TOOLWINDOW = 0x00000080; // keeps it out of Alt-Tab
    private const int WS_EX_TOPMOST = 0x00000008;

    private readonly Label _label;
    private readonly OverlayPosition _position;

    internal Overlay(OverlayPosition position)
    {
        _position = position;

        FormBorderStyle = FormBorderStyle.None;
        ShowInTaskbar = false;
        TopMost = true;
        StartPosition = FormStartPosition.Manual;
        BackColor = Color.FromArgb(24, 24, 27);
        Opacity = 0.92;
        Size = new Size(190, 44);

        _label = new Label
        {
            Dock = DockStyle.Fill,
            TextAlign = ContentAlignment.MiddleCenter,
            ForeColor = Color.White,
            Font = new Font("Segoe UI", 10f, FontStyle.Regular),
        };
        Controls.Add(_label);
    }

    protected override bool ShowWithoutActivation => true;

    protected override CreateParams CreateParams
    {
        get
        {
            var parameters = base.CreateParams;
            parameters.ExStyle |= WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
            return parameters;
        }
    }

    internal void ShowState(SessionState state)
    {
        switch (state)
        {
            case SessionState.Idle:
                Hide();
                return;

            case SessionState.Recording:
                _label.Text = "● recording";
                _label.ForeColor = Color.FromArgb(248, 113, 113);
                break;

            case SessionState.Transcribing:
                _label.Text = "… transcribing";
                _label.ForeColor = Color.FromArgb(147, 197, 253);
                break;
        }

        Reposition();

        if (!Visible)
        {
            Show();
        }

        // Re-assert topmost on every appearance. Another application going
        // full-screen or topmost while dictate was idle can otherwise leave the
        // indicator behind it, which is exactly when it is needed.
        BringToFront();
    }

    /// <summary>
    /// Places the indicator. Corner positions use the **main** screen's working
    /// area, so it sits above the taskbar and stays in one place across a
    /// multi-monitor desktop rather than wandering with the mouse.
    /// </summary>
    private void Reposition()
    {
        const int margin = 16;

        if (_position == OverlayPosition.NearCursor)
        {
            var cursor = Cursor.Position;
            var near = Screen.FromPoint(cursor).WorkingArea;

            var cx = Math.Min(cursor.X + 20, near.Right - Width - 8);
            var cy = Math.Min(cursor.Y + 28, near.Bottom - Height - 8);
            Location = new Point(Math.Max(near.Left + 8, cx), Math.Max(near.Top + 8, cy));
            return;
        }

        // WorkingArea rather than Bounds: it excludes the taskbar, so the
        // indicator is never hidden behind it.
        var screen = (Screen.PrimaryScreen ?? Screen.AllScreens[0]).WorkingArea;

        var x = _position is OverlayPosition.BottomRight or OverlayPosition.TopRight
            ? screen.Right - Width - margin
            : screen.Left + margin;

        var y = _position is OverlayPosition.BottomRight or OverlayPosition.BottomLeft
            ? screen.Bottom - Height - margin
            : screen.Top + margin;

        Location = new Point(x, y);
    }
}
