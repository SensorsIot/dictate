using System.Diagnostics;
using Dictate.Core;

namespace Dictate.Windows;

/// <summary>
/// The tray application: owns the session state machine of FSD §5 and wires the
/// hotkey, the recorder, the pipeline and delivery together.
///
/// One rule governs the layout of this class: **nothing slow may run inside the
/// keyboard hook callback.** A WH_KEYBOARD_LL callback blocks input processing
/// for the whole system until it returns, and Windows stops calling a hook that
/// exceeds LowLevelHooksTimeout (300 ms by default). So the hook handlers do
/// nothing but stamp the time and post to the message queue; the actual work
/// happens on the next pump, still on the UI thread, with the key event already
/// released back to the system.
/// </summary>
internal sealed class DictateApp : ApplicationContext
{
    /// <summary>How many delivered utterances stay recoverable. In memory only (FR-17.2).</summary>
    private const int RecentCapacity = 5;

    private readonly DictateConfig _config;
    private readonly DictationPipeline _pipeline;
    private readonly AudioRecorder _recorder;
    private readonly HotkeyListener _hotkey;
    private readonly Overlay _overlay;
    private readonly Feedback _feedback;
    private readonly TrayIcons _icons;
    private readonly NotifyIcon _tray;
    private readonly ToolStripMenuItem _recentMenu;
    private readonly RecentUtterances _recent = new(RecentCapacity);

    private SessionState _state = SessionState.Idle;
    private IntPtr _pinnedWindow;
    private long _pressedAtTicks;
    private TimeSpan _startLatency;

    internal DictateApp(DictateConfig config, DictationPipeline pipeline)
    {
        _config = config;
        _pipeline = pipeline;

        _icons = new TrayIcons();
        _overlay = new Overlay();
        _feedback = new Feedback(config.PlaySounds);
        _recorder = new AudioRecorder(config.MaximumRecordingSeconds, config.KeepMicrophoneOpen);
        _recorder.LimitReached += OnRecordingLimitReached;

        _recentMenu = new ToolStripMenuItem("Recent") { Enabled = false };

        var menu = new ContextMenuStrip();
        menu.Items.Add(_recentMenu);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Open config folder", null, (_, _) => OpenConfigFolder());
        menu.Items.Add("Re-enter API keys…", null, (_, _) => ReAuthenticate());
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add("Quit", null, (_, _) => Quit());

        _tray = new NotifyIcon
        {
            Icon = _icons.For(SessionState.Idle),
            Text = "dictate — idle",
            Visible = true,
            ContextMenuStrip = menu,
        };

        PreWarm();

        // Installed last: once the hook is live the callbacks can arrive, and
        // they assume everything above exists.
        _hotkey = new HotkeyListener(config.HotkeyVirtualKey, config.SuppressHotkey);
        _hotkey.Pressed += OnHotkeyPressed;
        _hotkey.Released += OnHotkeyReleased;
    }

    /// <summary>
    /// Pays every first-use cost at startup rather than on the first press.
    /// Creating the overlay's window handle, loading winmm and touching the
    /// process APIs each cost tens to hundreds of milliseconds once, and the
    /// user is waiting for all of them if they happen on press.
    /// </summary>
    private void PreWarm()
    {
        _ = _overlay.Handle;          // create the window now, not mid-utterance
        _recorder.PreWarm();

        try
        {
            // First touch of System.Diagnostics.Process is expensive; do it here.
            using var self = Process.GetCurrentProcess();
            _ = self.ProcessName;
        }
        catch (Exception)
        {
            // Diagnostics only.
        }
    }

    // --- state ---------------------------------------------------------------

    private void SetState(SessionState state)
    {
        _state = state;
        _tray.Icon = _icons.For(state);
        _tray.Text = state switch
        {
            SessionState.Recording => "dictate — recording",
            SessionState.Transcribing => "dictate — transcribing",
            _ => "dictate — idle",
        };

        if (_config.ShowOverlay)
        {
            _overlay.ShowState(state);
        }
    }

    // --- hotkey: these two run INSIDE the hook. Keep them trivial. -----------

    private void OnHotkeyPressed()
    {
        var pressedAt = Stopwatch.GetTimestamp();
        _overlay.BeginInvoke(() => BeginSession(pressedAt));
    }

    private void OnHotkeyReleased()
    {
        _overlay.BeginInvoke(EndSession);
    }

    // --- session, on the message loop ---------------------------------------

    private void BeginSession(long pressedAt)
    {
        // FR-5.1: one session at a time. A press while transcribing is ignored
        // rather than queued — two utterances in flight is how audio from one
        // ends up delivered into the target of the other.
        if (_state != SessionState.Idle)
        {
            return;
        }

        if (!AudioRecorder.HasInputDevice)
        {
            _feedback.Error();
            Notify("No microphone", "dictate found no audio input device.");
            return;
        }

        // FR-6.1: the target window is recorded now and never re-read. Only the
        // handle — resolving it to a process name costs a process lookup, and
        // that can wait until the pipeline is already running.
        _pinnedWindow = WindowInspector.Foreground();
        _pressedAtTicks = pressedAt;

        try
        {
            _recorder.Start();
        }
        catch (Exception ex)
        {
            _feedback.Error();
            Notify("Could not start recording", ex.Message);
            return;
        }

        _startLatency = Stopwatch.GetElapsedTime(pressedAt);

        SetState(SessionState.Recording);
        _feedback.Start();
    }

    private async void EndSession()
    {
        if (_state != SessionState.Recording)
        {
            return;
        }

        var held = Stopwatch.GetElapsedTime(_pressedAtTicks);
        var pcm = _recorder.Stop();

        // FR-5.2: an accidental tap costs nothing and says nothing.
        if (held.TotalMilliseconds < _config.MinimumHoldMs)
        {
            SetState(SessionState.Idle);
            return;
        }

        SetState(SessionState.Transcribing);
        _feedback.Stop();

        // Deferred from press time: this is a process lookup, and here it runs
        // while the user is already waiting for the network anyway.
        var target = WindowInspector.Describe(_pinnedWindow, _config.ExtraConsoleProcesses);

        Utterance utterance;
        try
        {
            utterance = await _pipeline.ProcessAsync(pcm, target);
        }
        catch (Exception ex)
        {
            // The pipeline degrades internally; reaching here means something
            // outside its contract broke. Never let it kill the message loop.
            utterance = Utterance.Fail(ex.Message);
        }

        SetState(SessionState.Idle);

        try
        {
            Deliver(utterance, target);
        }
        catch (Exception ex)
        {
            // This method is async void: an escaping exception is posted to the
            // UI thread and takes the process down with it. A bug in delivery
            // should cost one utterance, not the running session — the user has
            // no other way to get the text back.
            _feedback.Error();
            Notify("Delivery failed", ex.Message);
        }
    }

    private void OnRecordingLimitReached()
    {
        // Raised on the recorder's callback thread.
        if (_overlay.IsHandleCreated)
        {
            _overlay.BeginInvoke(() =>
            {
                _feedback.Error();
                Notify("Recording limit reached",
                    $"Stopped after {_config.MaximumRecordingSeconds}s. Transcribing what was captured.");
            });
        }
    }

    // --- delivery ------------------------------------------------------------

    private void Deliver(Utterance utterance, TargetContext target)
    {
        if (utterance.Status == UtteranceStatus.Failed)
        {
            _feedback.Error();
            Notify("Dictation failed", utterance.Error ?? "Unknown error.");
            return;
        }

        if (!utterance.HasText)
        {
            return;
        }

        Remember(utterance);

        if (utterance.Status == UtteranceStatus.CleanupFailed)
        {
            // Delivered verbatim rather than lost (FR-7.1), but the user is told
            // so an unusually rough sentence is explained rather than puzzling.
            Notify("Delivered without cleanup", utterance.Error ?? "Cleanup failed.");
        }
        else if (_config.ShowTimings)
        {
            Notify("Timings",
                $"press→recording {_startLatency.TotalMilliseconds:0} ms · " +
                $"transcribe {utterance.TranscribeTime.TotalMilliseconds:0} ms · " +
                $"cleanup {utterance.CleanupTime.TotalMilliseconds:0} ms");
        }

        // FR-6.2: the window that had focus at press time, or nothing.
        if (WindowInspector.Foreground() != _pinnedWindow)
        {
            ToClipboard(utterance.Text, "Focus changed", "The text is on the clipboard — press Ctrl+V.");
            return;
        }

        // A trailing space so consecutive dictations do not run together. Not
        // added to the clipboard path, where the user places the text and can
        // see exactly what they are pasting.
        var typed = _config.AppendSpaceAfterInsert && !target.IsConsole
            ? utterance.Text + " "
            : utterance.Text;

        try
        {
            TextInjector.Type(typed, _config.InjectionChunkSize, _config.InjectionChunkDelayMs);
        }
        catch (Exception ex)
        {
            ToClipboard(utterance.Text, "Could not type the text", ex.Message);
        }
    }

    private void ToClipboard(string text, string title, string detail)
    {
        try
        {
            Clipboard.SetText(text);
            Notify(title, detail);
        }
        catch (Exception ex)
        {
            _feedback.Error();
            Notify("Text could not be delivered", $"{detail} Clipboard also failed: {ex.Message}");
        }
    }

    // --- recent utterances (memory only) -------------------------------------

    private void Remember(Utterance utterance)
    {
        _recent.Add(utterance);

        // Snapshot, detach, then dispose — in that order. ToolStripItem.Dispose
        // removes the item from its parent's DropDownItems, so disposing while
        // enumerating that collection modifies it mid-enumeration and throws.
        var previous = _recentMenu.DropDownItems.Cast<ToolStripItem>().ToArray();
        _recentMenu.DropDownItems.Clear();

        foreach (var item in previous)
        {
            item.Dispose();
        }

        foreach (var item in _recent.Items)
        {
            var label = item.Text.Length > 60 ? item.Text[..60] + "…" : item.Text;
            var text = item.Text;
            _recentMenu.DropDownItems.Add(label, null, (_, _) => Clipboard.SetText(text));
        }

        _recentMenu.Enabled = _recentMenu.DropDownItems.Count > 0;
    }

    // --- menu ----------------------------------------------------------------

    private static void OpenConfigFolder()
    {
        var folder = Paths.ConfigFolder;
        Directory.CreateDirectory(folder);
        Process.Start(new ProcessStartInfo(folder) { UseShellExecute = true });
    }

    private void ReAuthenticate()
    {
        if (AuthForm.Prompt())
        {
            Notify("Keys stored", "Restart dictate for the new keys to take effect.");
        }
    }

    private void Notify(string title, string detail) =>
        _tray.ShowBalloonTip(4000, title, detail, ToolTipIcon.None);

    private void Quit()
    {
        _tray.Visible = false;
        ExitThread();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _hotkey.Dispose();
            _recorder.Dispose();
            _feedback.Dispose();
            _tray.Dispose();
            _icons.Dispose();
            _overlay.Dispose();
            _recent.Clear(); // FR-17.2: nothing outlives the process
        }

        base.Dispose(disposing);
    }
}
