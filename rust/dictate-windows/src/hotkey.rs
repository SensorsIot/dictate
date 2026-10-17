//! A WH_KEYBOARD_LL hook watching one key, reporting press and release to the
//! main window as posted messages.
//!
//! The hook callback runs on the thread that installed it, and only while that
//! thread pumps messages — so it is installed from the UI thread, and its state
//! lives in a thread-local. **Nothing slow may run inside the callback:** it
//! blocks input for the whole system until it returns, and Windows unhooks a
//! callback that exceeds LowLevelHooksTimeout. So it stamps the time, posts a
//! message, and returns.
//!
//! **A press is not guaranteed to produce a release.** A low-level hook does not
//! receive input destined for a higher-integrity window (UIPI). Hold the hotkey,
//! let an elevated window take focus, release it there, and the WM_KEYUP never
//! arrives here — for a push-to-talk, an open microphone for the rest of the
//! day. So `reconcile`, driven by a timer, checks the hook's idea of the key
//! against the physical state and synthesises the release the hook never got.

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, SystemParametersInfoW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::injector::INJECTION_SIGNATURE;
use crate::win::{SendHwnd, WM_HOTKEY_DOWN, WM_HOTKEY_REJECTED, WM_HOTKEY_UP};

/// Added to a measured interval to absorb scheduling jitter.
const MARGIN_MS: u64 = 150;

/// How often `reconcile` should be called. 250 ms is well below the point
/// where a stuck microphone is noticeable, and GetAsyncKeyState is one call.
pub const RECONCILE_INTERVAL_MS: u32 = 250;

// KBDLLHOOKSTRUCT.flags. "Injected" does not mean illegitimate: remote-desktop
// tools deliver the user's real keystrokes this way too.
const LLKHF_INJECTED: u32 = 0x10;
const LLKHF_LOWER_IL_INJECTED: u32 = 0x02;

const SPI_GETKEYBOARDSPEED: u32 = 0x000A;
const SPI_GETKEYBOARDDELAY: u32 = 0x0016;

/// wparam bits of WM_HOTKEY_DOWN / WM_HOTKEY_REJECTED.
pub const ORIGIN_INJECTED: usize = 1;
pub const ORIGIN_LOWER_INTEGRITY: usize = 2;

/// wparam of WM_HOTKEY_UP.
pub const RELEASE_KEY_UP: usize = 0;
pub const RELEASE_RECONCILED: usize = 1;

struct Hook {
    handle: HHOOK,
    notify: SendHwnd,
    virtual_key: u32,
    suppress: bool,
    ignore_injected: bool,
    initial_grace: Duration,
    repeat_grace: Duration,
    is_down: bool,
    repeat_seen: bool,
    last_key_down: Instant,
    pressed_at: Instant,
    /// Whether this hook swallowed the key-down of the press in progress, and
    /// therefore owns the matching key-up.
    ///
    /// Suppression must be symmetric or it corrupts the system's idea of the
    /// keyboard. Install the hook while the key is already held — every restart
    /// mid-press — and the down has already reached the system. Swallowing the
    /// up then leaves Windows believing the key is held for the rest of the
    /// session: with a modifier, every later keystroke becomes a chord.
    owns_key_up: bool,
}

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

/// The two graces in force, for the startup log.
pub struct Graces {
    pub initial: Duration,
    pub repeat: Duration,
}

/// Installs the hook on the calling thread, which must pump messages.
pub fn install(virtual_key: i32, suppress: bool, ignore_injected: bool, notify: HWND) -> Result<Graces, String> {
    // Two graces, because the silence a held key produces has two lengths.
    // Before the first repeat there is a genuine gap of the initial delay;
    // after it, repeats arrive steadily and a few missed ones are conclusive.
    let (initial_delay_ms, repeat_period_ms) = keyboard_timing();
    let initial_grace = Duration::from_millis(initial_delay_ms + MARGIN_MS);
    let repeat_grace = Duration::from_millis(MARGIN_MS.max(repeat_period_ms * 4 + MARGIN_MS));

    HOOK.with(|cell| {
        *cell.borrow_mut() = Some(Hook {
            handle: null_mut(),
            notify: SendHwnd::new(notify),
            virtual_key: virtual_key as u32,
            suppress,
            ignore_injected,
            initial_grace,
            repeat_grace,
            is_down: false,
            repeat_seen: false,
            last_key_down: Instant::now(),
            pressed_at: Instant::now(),
            owns_key_up: false,
        });
    });

    let handle = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(null()), 0) };
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        HOOK.with(|cell| cell.borrow_mut().take());
        return Err(format!("Could not install the keyboard hook (Win32 error {error})."));
    }

    HOOK.with(|cell| {
        if let Some(hook) = cell.borrow_mut().as_mut() {
            hook.handle = handle;
        }
    });

    Ok(Graces { initial: initial_grace, repeat: repeat_grace })
}

pub fn uninstall() {
    if let Some(hook) = HOOK.with(|cell| cell.borrow_mut().take()) {
        if !hook.handle.is_null() {
            unsafe { UnhookWindowsHookEx(hook.handle) };
        }
    }
}

/// When the press now in progress was seen by the hook.
pub fn pressed_at() -> Option<Instant> {
    HOOK.with(|cell| cell.try_borrow().ok().and_then(|hook| hook.as_ref().map(|h| h.pressed_at)))
}

/// Reads the user's actual auto-repeat timing rather than assuming the slowest
/// Windows permits: the worst case is a 1000 ms delay at 2.5 repeats per second,
/// a typical machine runs 500 ms at 30 per second.
fn keyboard_timing() -> (u64, u64) {
    // The pessimistic pair, used if either query fails.
    let mut delay_index = 3;
    let mut speed_index = 0;

    let mut value = 0i32;
    if unsafe { SystemParametersInfoW(SPI_GETKEYBOARDDELAY, 0, &mut value as *mut i32 as *mut c_void, 0) } != 0 {
        delay_index = value.clamp(0, 3);
    }

    value = 0;
    if unsafe { SystemParametersInfoW(SPI_GETKEYBOARDSPEED, 0, &mut value as *mut i32 as *mut c_void, 0) } != 0 {
        speed_index = value.clamp(0, 31);
    }

    let initial_delay_ms = 250 * (delay_index as u64 + 1);
    let repeats_per_second = 2.5 + 27.5 * f64::from(speed_index) / 31.0;

    (initial_delay_ms, (1000.0 / repeats_per_second).ceil() as u64)
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A panic must not unwind into user32; treat it as "not ours".
    let swallow = panic::catch_unwind(AssertUnwindSafe(|| handle(code, wparam, lparam))).unwrap_or(false);

    if swallow {
        1
    } else {
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

/// Returns true to swallow the event.
fn handle(code: i32, wparam: WPARAM, lparam: LPARAM) -> bool {
    if code != 0 {
        return false; // not HC_ACTION
    }

    let data = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };

    HOOK.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else { return false };
        let Some(hook) = guard.as_mut() else { return false };

        // Ignore our own synthetic keystrokes, or typing a sentence containing
        // the hotkey's character would re-trigger dictation.
        if data.dwExtraInfo == INJECTION_SIGNATURE || data.vkCode != hook.virtual_key {
            return false;
        }

        let mut origin = 0;
        if data.flags & LLKHF_INJECTED != 0 {
            origin |= ORIGIN_INJECTED;
        }
        if data.flags & LLKHF_LOWER_IL_INJECTED != 0 {
            origin |= ORIGIN_LOWER_INTEGRITY;
        }

        let message = wparam as u32;

        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            // Stamped for every down-event including auto-repeat, because the
            // repeats are what prove the key is still held.
            hook.last_key_down = Instant::now();

            if hook.is_down {
                // An auto-repeat; the first lets reconcile use the short grace.
                hook.repeat_seen = true;
            } else {
                // Rejecting injected presses is off by default: remote-desktop
                // tools deliver real keystrokes as injected input. Passed
                // through rather than swallowed — suppressing a down whose up we
                // will not suppress is what breaks the system's key state.
                if origin & ORIGIN_INJECTED != 0 && hook.ignore_injected {
                    hook.notify.post(WM_HOTKEY_REJECTED, origin, 0);
                    return false;
                }

                hook.is_down = true;
                hook.repeat_seen = false;
                hook.owns_key_up = hook.suppress;
                hook.pressed_at = Instant::now();
                hook.notify.post(WM_HOTKEY_DOWN, origin, 0);
            }
        } else if message == WM_KEYUP || message == WM_SYSKEYUP {
            let owned = hook.owns_key_up;
            hook.owns_key_up = false;

            if hook.is_down {
                hook.is_down = false;
                hook.notify.post(WM_HOTKEY_UP, RELEASE_KEY_UP, 0);
            }

            return owned;
        }

        hook.suppress
    })
}

/// The safety net for a missed WM_KEYUP. Called from a timer on the UI thread,
/// so it cannot race the hook callback.
///
/// Two signals must agree before a release is declared. GetAsyncKeyState
/// reports what the system holds — but a suppressed key never reaches the
/// system, so for a swallowed hotkey it reports "up" the whole time it is held.
/// Auto-repeat is the signal that survives suppression: the hook keeps getting
/// down-events while the key is held. Silence from the keyboard is what a
/// release looks like from in here.
pub fn reconcile() {
    HOOK.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else { return };
        let Some(hook) = guard.as_mut() else { return };

        if !hook.is_down {
            return;
        }

        let state = unsafe { GetAsyncKeyState(hook.virtual_key as i32) } as u16;
        if (state & 0x8000) != 0 {
            return;
        }

        let grace = if hook.repeat_seen { hook.repeat_grace } else { hook.initial_grace };
        if hook.last_key_down.elapsed() < grace {
            return;
        }

        hook.is_down = false;
        hook.notify.post(WM_HOTKEY_UP, RELEASE_RECONCILED, 0);
    });
}
