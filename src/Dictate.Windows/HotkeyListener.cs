using System.Runtime.InteropServices;

namespace Dictate.Windows;

/// <summary>Why a hotkey release was reported.</summary>
internal enum ReleaseReason
{
    /// <summary>A WM_KEYUP arrived through the hook — the normal path.</summary>
    KeyUp,

    /// <summary>
    /// No WM_KEYUP ever arrived, but the key is physically up. See
    /// <see cref="HotkeyListener"/> for how that happens.
    /// </summary>
    Reconciled,
}

/// <summary>Where a hotkey press came from.</summary>
/// <param name="Injected">Synthesised by some process rather than a physical key.</param>
/// <param name="LowerIntegrity">Injected by a process at a lower integrity level than ours.</param>
internal readonly record struct PressOrigin(bool Injected, bool LowerIntegrity);

/// <summary>
/// A WH_KEYBOARD_LL hook watching one key, reporting press and release.
///
/// The hook callback runs on the thread that installed it, and only while that
/// thread pumps messages — so this must be created from the UI thread, and the
/// delegate must be kept alive for the lifetime of the hook or the GC will
/// collect it out from under Windows.
///
/// **A press is not guaranteed to produce a release.** A low-level hook does not
/// receive input destined for a higher-integrity window (UIPI). Hold the hotkey,
/// let an elevated window take focus, release it there, and the WM_KEYUP is
/// never delivered here: <c>_isDown</c> stays true, no release is reported, and
/// whatever the consumer started on press runs until the process exits. For a
/// push-to-talk that opens the microphone, that means an open microphone for the
/// rest of the day.
///
/// So the hook is not treated as the only source of truth. A timer reconciles
/// <c>_isDown</c> against the physical key state and synthesises the release the
/// hook never delivered.
/// </summary>
internal sealed class HotkeyListener : IDisposable
{
    /// <summary>
    /// How often the physical key state is checked against what the hook told us.
    /// 250 ms is well below the point where a stuck microphone is noticeable and
    /// far too cheap to matter — GetAsyncKeyState is a single user32 call.
    /// </summary>
    private const int ReconcileIntervalMs = 250;

    /// <summary>Added to a measured interval to absorb scheduling jitter.</summary>
    private const int MarginMs = 150;

    private readonly int _virtualKey;
    private readonly bool _suppress;
    private readonly bool _ignoreInjected;
    private readonly Interop.LowLevelKeyboardProc _callback; // field, not a local: keeps it rooted
    private readonly System.Windows.Forms.Timer _reconcile;
    private readonly int _initialGraceMs;
    private readonly int _repeatGraceMs;
    private IntPtr _hook;
    private bool _isDown;
    private bool _repeatSeen;
    private long _lastKeyDownTicks;

    /// <summary>
    /// Whether this hook swallowed the key-down of the press now in progress, and
    /// therefore owns the matching key-up.
    ///
    /// Suppression must be symmetric or it corrupts the system's idea of the
    /// keyboard. Install the hook while the key is already held — which happens
    /// every time the application is restarted mid-press — and the down has
    /// already reached the system. Swallowing the up then leaves Windows
    /// believing the key is held for the rest of the session: with a modifier
    /// that turns every subsequent keystroke into a chord, and the user's
    /// keyboard appears to stop working.
    /// </summary>
    private bool _ownsKeyUp;

    /// <summary>The two graces actually in force, for the startup log.</summary>
    public (int Initial, int Repeat) Grace => (_initialGraceMs, _repeatGraceMs);

    /// <summary>
    /// Reads the user's actual auto-repeat timing rather than assuming the
    /// slowest Windows permits. The difference is not academic: the worst case is
    /// a 1000 ms initial delay at 2.5 repeats per second, while a typical machine
    /// runs 500 ms at 30 per second — so a fixed worst-case grace is roughly four
    /// times longer than it needs to be on most machines.
    /// </summary>
    private static (int InitialDelayMs, int RepeatPeriodMs) ReadKeyboardTiming()
    {
        // The pessimistic pair, used if either query fails.
        var delayIndex = 3;
        var speedIndex = 0;

        var value = 0;
        if (Interop.SystemParametersInfoW(Interop.SPI_GETKEYBOARDDELAY, 0, ref value, 0))
        {
            delayIndex = Math.Clamp(value, 0, 3);
        }

        value = 0;
        if (Interop.SystemParametersInfoW(Interop.SPI_GETKEYBOARDSPEED, 0, ref value, 0))
        {
            speedIndex = Math.Clamp(value, 0, 31);
        }

        var initialDelayMs = 250 * (delayIndex + 1);
        var repeatsPerSecond = 2.5 + (27.5 * speedIndex / 31.0);

        return (initialDelayMs, (int)Math.Ceiling(1000.0 / repeatsPerSecond));
    }

    public event Action<PressOrigin>? Pressed;
    public event Action<ReleaseReason>? Released;

    /// <summary>Raised for a hotkey event that was rejected as injected (FR-8.6).</summary>
    public event Action<PressOrigin>? InjectedRejected;

    public HotkeyListener(int virtualKey, bool suppress, bool ignoreInjected)
    {
        _virtualKey = virtualKey;
        _suppress = suppress;
        _ignoreInjected = ignoreInjected;
        _callback = HookProc;

        // Two graces, because the silence a held key produces has two lengths.
        // Before the first repeat there is a genuine gap of the initial delay;
        // after it, repeats arrive steadily and a few missed ones are already
        // conclusive. Waiting the long one for the whole hold would be four times
        // longer than necessary for all but the first half second.
        var (initialDelayMs, repeatPeriodMs) = ReadKeyboardTiming();
        _initialGraceMs = initialDelayMs + MarginMs;
        _repeatGraceMs = Math.Max(MarginMs, (repeatPeriodMs * 4) + MarginMs);

        _hook = Interop.SetWindowsHookExW(Interop.WH_KEYBOARD_LL, _callback, Interop.GetModuleHandleW(null), 0);
        if (_hook == IntPtr.Zero)
        {
            throw new InvalidOperationException(
                $"Could not install the keyboard hook (Win32 error {Marshal.GetLastWin32Error()}).");
        }

        // A Forms timer, so the tick lands on the UI thread — the same thread the
        // hook callback runs on. That is what makes _isDown safe to touch from
        // both places without a lock.
        _reconcile = new System.Windows.Forms.Timer { Interval = ReconcileIntervalMs };
        _reconcile.Tick += (_, _) => Reconcile();
        _reconcile.Start();
    }

    private IntPtr HookProc(int nCode, IntPtr wParam, IntPtr lParam)
    {
        if (nCode != Interop.HC_ACTION)
        {
            return Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
        }

        var data = Marshal.PtrToStructure<Interop.KBDLLHOOKSTRUCT>(lParam);

        // Ignore our own synthetic keystrokes, or typing a sentence containing
        // the hotkey's character would re-trigger dictation.
        if (data.dwExtraInfo == Interop.InjectionSignature || data.vkCode != (uint)_virtualKey)
        {
            return Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
        }

        var origin = new PressOrigin(
            Injected: (data.flags & Interop.LLKHF_INJECTED) != 0,
            LowerIntegrity: (data.flags & Interop.LLKHF_LOWER_IL_INJECTED) != 0);

        var message = (int)wParam;

        if (message is Interop.WM_KEYDOWN or Interop.WM_SYSKEYDOWN)
        {
            // Stamped for every down-event including auto-repeat, because the
            // repeats are what prove the key is still held. See Reconcile.
            _lastKeyDownTicks = System.Diagnostics.Stopwatch.GetTimestamp();

            // A down-event while already down is an auto-repeat, and the first one
            // is what lets the reconciler switch to the short grace.
            if (_isDown)
            {
                _repeatSeen = true;
            }

            // Auto-repeat fires KEYDOWN continuously while held; only the edge matters.
            if (!_isDown)
            {
                // Another process synthesised our hotkey. Rejecting this is off by
                // default and deliberately so: remote-desktop tools deliver the
                // user's real keystrokes as injected input, so refusing them would
                // silently break dictation over AnyDesk, TeamViewer or RDP. The
                // event is always reported, so the log names the source either way.
                if (origin.Injected && _ignoreInjected)
                {
                    // Passed through rather than swallowed: this press is not ours
                    // to consume, and suppressing a down whose up we will not
                    // suppress is what breaks the system's key state.
                    InjectedRejected?.Invoke(origin);
                    return Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
                }

                _isDown = true;
                _repeatSeen = false;
                _ownsKeyUp = _suppress;
                Pressed?.Invoke(origin);
            }
        }
        else if (message is Interop.WM_KEYUP or Interop.WM_SYSKEYUP)
        {
            // Suppressed only if the matching down was suppressed too. See
            // _ownsKeyUp for what asymmetry costs the user.
            var owned = _ownsKeyUp;
            _ownsKeyUp = false;

            if (_isDown)
            {
                _isDown = false;
                Released?.Invoke(ReleaseReason.KeyUp);
            }

            return owned
                ? new IntPtr(1)
                : Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
        }

        // Suppression is off by default. Right Ctrl on its own does nothing in
        // almost every application, whereas swallowing it breaks every Ctrl+key
        // combination the user makes with that hand.
        return _suppress
            ? new IntPtr(1)
            : Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
    }

    /// <summary>
    /// The safety net for the missed WM_KEYUP described on the class. Runs on the
    /// UI thread, so it cannot race the hook callback.
    ///
    /// Two independent signals must agree before a release is declared, and the
    /// second one is not optional. <c>GetAsyncKeyState</c> reports the state the
    /// system holds — but a suppressed hotkey never reaches the system, so for
    /// any key the user has asked to swallow it reports "up" the entire time the
    /// key is held. On its own it therefore ends a suppressed session at the
    /// first tick, the next auto-repeat starts a fresh one, and a three-second
    /// hold becomes eleven sessions of a quarter-second each, every one of them
    /// discarded as an accidental tap.
    ///
    /// Auto-repeat is the signal that survives suppression: the hook keeps
    /// receiving down-events for as long as the key is held, whether or not they
    /// are passed on. Silence from the keyboard is what a release actually looks
    /// like from in here.
    /// </summary>
    private void Reconcile()
    {
        if (!_isDown)
        {
            return;
        }

        // High bit set means currently down. Trustworthy only when the key is
        // passed through — never a reason on its own to declare a release.
        if ((Interop.GetAsyncKeyState(_virtualKey) & 0x8000) != 0)
        {
            return;
        }

        // Before the first repeat, the silence of the initial delay is expected
        // and proves nothing. After it, four missed repeats do.
        var grace = _repeatSeen ? _repeatGraceMs : _initialGraceMs;

        if (System.Diagnostics.Stopwatch.GetElapsedTime(_lastKeyDownTicks).TotalMilliseconds < grace)
        {
            return;
        }

        _isDown = false;
        Released?.Invoke(ReleaseReason.Reconciled);
    }

    public void Dispose()
    {
        _reconcile.Stop();
        _reconcile.Dispose();

        if (_hook != IntPtr.Zero)
        {
            Interop.UnhookWindowsHookEx(_hook);
            _hook = IntPtr.Zero;
        }
    }
}
