using System.Runtime.InteropServices;

namespace Dictate.Windows;

/// <summary>
/// A WH_KEYBOARD_LL hook watching one key, reporting press and release.
///
/// The hook callback runs on the thread that installed it, and only while that
/// thread pumps messages — so this must be created from the UI thread, and the
/// delegate must be kept alive for the lifetime of the hook or the GC will
/// collect it out from under Windows.
/// </summary>
internal sealed class HotkeyListener : IDisposable
{
    private readonly int _virtualKey;
    private readonly bool _suppress;
    private readonly Interop.LowLevelKeyboardProc _callback; // field, not a local: keeps it rooted
    private IntPtr _hook;
    private bool _isDown;

    public event Action? Pressed;
    public event Action? Released;

    public HotkeyListener(int virtualKey, bool suppress)
    {
        _virtualKey = virtualKey;
        _suppress = suppress;
        _callback = HookProc;

        _hook = Interop.SetWindowsHookExW(Interop.WH_KEYBOARD_LL, _callback, Interop.GetModuleHandleW(null), 0);
        if (_hook == IntPtr.Zero)
        {
            throw new InvalidOperationException(
                $"Could not install the keyboard hook (Win32 error {Marshal.GetLastWin32Error()}).");
        }
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

        var message = (int)wParam;

        if (message is Interop.WM_KEYDOWN or Interop.WM_SYSKEYDOWN)
        {
            // Auto-repeat fires KEYDOWN continuously while held; only the edge matters.
            if (!_isDown)
            {
                _isDown = true;
                Pressed?.Invoke();
            }
        }
        else if (message is Interop.WM_KEYUP or Interop.WM_SYSKEYUP)
        {
            if (_isDown)
            {
                _isDown = false;
                Released?.Invoke();
            }
        }

        // Suppression is off by default. Right Ctrl on its own does nothing in
        // almost every application, whereas swallowing it breaks every Ctrl+key
        // combination the user makes with that hand.
        return _suppress
            ? new IntPtr(1)
            : Interop.CallNextHookEx(_hook, nCode, wParam, lParam);
    }

    public void Dispose()
    {
        if (_hook != IntPtr.Zero)
        {
            Interop.UnhookWindowsHookEx(_hook);
            _hook = IntPtr.Zero;
        }
    }
}
