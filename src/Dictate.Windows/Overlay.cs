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

    internal Overlay()
    {
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

        PositionNearCursor();

        if (!Visible)
        {
            Show();
        }
    }

    private void PositionNearCursor()
    {
        var cursor = Cursor.Position;
        var screen = Screen.FromPoint(cursor).WorkingArea;

        // Below-right of the cursor, nudged back inside the working area so it
        // is never half off a monitor edge.
        var x = Math.Min(cursor.X + 20, screen.Right - Width - 8);
        var y = Math.Min(cursor.Y + 28, screen.Bottom - Height - 8);

        Location = new Point(Math.Max(screen.Left + 8, x), Math.Max(screen.Top + 8, y));
    }
}
