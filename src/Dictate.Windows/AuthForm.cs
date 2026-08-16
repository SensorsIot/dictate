namespace Dictate.Windows;

/// <summary>
/// Collects the two API keys and stores them in Credential Manager.
///
/// A dialog rather than a console prompt because this ships as a WinExe: it has
/// no console to read from, and attaching one just to ask two questions is more
/// moving parts than the question deserves.
/// </summary>
internal sealed class AuthForm : Form
{
    private readonly TextBox _elevenLabs;
    private readonly TextBox _anthropic;

    private AuthForm()
    {
        Text = "dictate — API keys";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        StartPosition = FormStartPosition.CenterScreen;
        MinimizeBox = false;
        MaximizeBox = false;
        ClientSize = new Size(460, 210);

        var intro = new Label
        {
            Text = "Keys are stored in Windows Credential Manager, encrypted under your\n" +
                   "Windows account. They are never written to a file.",
            Location = new Point(12, 12),
            Size = new Size(436, 34),
        };

        var elevenLabsLabel = new Label { Text = "ElevenLabs key", Location = new Point(12, 58), Size = new Size(120, 20) };
        _elevenLabs = new TextBox
        {
            Location = new Point(140, 55),
            Size = new Size(300, 23),
            UseSystemPasswordChar = true,
            Text = CredentialStore.Read(CredentialStore.ElevenLabsTarget) ?? "",
        };

        var anthropicLabel = new Label { Text = "Anthropic key", Location = new Point(12, 92), Size = new Size(120, 20) };
        _anthropic = new TextBox
        {
            Location = new Point(140, 89),
            Size = new Size(300, 23),
            UseSystemPasswordChar = true,
            Text = CredentialStore.Read(CredentialStore.AnthropicTarget) ?? "",
        };

        var note = new Label
        {
            Text = "Leave the Anthropic key blank to dictate without cleanup.",
            Location = new Point(140, 116),
            Size = new Size(300, 20),
            ForeColor = SystemColors.GrayText,
        };

        var save = new Button { Text = "Save", Location = new Point(264, 158), Size = new Size(85, 28), DialogResult = DialogResult.OK };
        var cancel = new Button { Text = "Cancel", Location = new Point(355, 158), Size = new Size(85, 28), DialogResult = DialogResult.Cancel };

        save.Click += OnSave;

        Controls.AddRange([intro, elevenLabsLabel, _elevenLabs, anthropicLabel, _anthropic, note, save, cancel]);
        AcceptButton = save;
        CancelButton = cancel;
    }

    private void OnSave(object? sender, EventArgs e)
    {
        var elevenLabs = _elevenLabs.Text.Trim();

        if (elevenLabs.Length == 0)
        {
            MessageBox.Show(this,
                "An ElevenLabs key is required — without it there is nothing to transcribe with.",
                "dictate", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            DialogResult = DialogResult.None;
            return;
        }

        try
        {
            CredentialStore.Write(CredentialStore.ElevenLabsTarget, elevenLabs);

            var anthropic = _anthropic.Text.Trim();
            if (anthropic.Length > 0)
            {
                CredentialStore.Write(CredentialStore.AnthropicTarget, anthropic);
            }
            else
            {
                CredentialStore.Delete(CredentialStore.AnthropicTarget);
            }
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "dictate", MessageBoxButtons.OK, MessageBoxIcon.Error);
            DialogResult = DialogResult.None;
        }
    }

    /// <summary>Shows the dialog. Returns true if keys were saved.</summary>
    internal static bool Prompt()
    {
        using var form = new AuthForm();
        return form.ShowDialog() == DialogResult.OK;
    }
}
