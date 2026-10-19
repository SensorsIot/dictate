//! FR-8.8: the Windows-key hotkey pressed in an administrator window.
//!
//! The keyboard hook never sees that press (UIPI), so it is not swallowed and
//! its release opens the Start menu. Nor is the key visible any other way: the
//! async key state reads up throughout. What is visible is the consequence —
//! the Start menu taking the foreground straight from an elevated window — so
//! that is what this watches for. It cannot tell which Windows key opened Start;
//! the left one, pressed on purpose in an elevated window, is closed too. That
//! trade is the user's decision.
//!
//! A click on the Start button is told apart by the cursor sitting on the
//! taskbar, which the mouse reaches through Explorer, not the elevated window.

use std::cell::Cell;
use std::mem::zeroed;
use std::panic::{self, AssertUnwindSafe};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GetCursorPos, WindowFromPoint};

use crate::win::{SendHwnd, WM_START_FROM_ELEVATED};
use crate::window_inspector;

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
const GA_ROOT: u32 = 2;

const VK_LWIN: i32 = 0x5B;
const VK_RWIN: i32 = 0x5C;

/// The Start menu's host: Windows 11, then Windows 10.
const START_PROCESSES: [&str; 2] = ["SearchHost", "StartMenuExperienceHost"];

thread_local! {
    static HOOK: Cell<usize> = const { Cell::new(0) };
    static NOTIFY: Cell<usize> = const { Cell::new(0) };
    static PREVIOUS_ELEVATED: Cell<bool> = const { Cell::new(false) };
}

/// Only for a Windows-key hotkey: with any other hotkey a Start menu opened
/// from an elevated window is the user's own doing.
pub fn install(hotkey_virtual_key: i32, notify: HWND) {
    if hotkey_virtual_key != VK_LWIN && hotkey_virtual_key != VK_RWIN {
        return;
    }

    NOTIFY.with(|cell| cell.set(notify as usize));

    // Out of context, so the callback runs on this (the UI) thread from its
    // message loop and no DLL is injected anywhere.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            zeroed(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    HOOK.with(|cell| cell.set(hook as usize));
}

pub fn uninstall() {
    let hook = HOOK.with(|cell| cell.replace(0));
    if hook != 0 {
        unsafe { UnhookWinEvent(hook as HWINEVENTHOOK) };
    }
}

/// Whether the Start menu is in front now. Used to retry the close once.
pub fn start_in_front() -> bool {
    is_start(window_inspector::foreground())
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| foreground_changed(window)));
}

fn foreground_changed(window: HWND) {
    if window.is_null() {
        return;
    }

    let came_from_elevated = PREVIOUS_ELEVATED.with(|cell| cell.replace(window_inspector::is_elevated(window)));

    if came_from_elevated && is_start(window) && !cursor_on_taskbar() {
        let notify = NOTIFY.with(Cell::get);
        SendHwnd::new(notify as HWND).post(WM_START_FROM_ELEVATED, 0, 0);
    }
}

fn is_start(window: HWND) -> bool {
    let process = window_inspector::process_name(window);
    START_PROCESSES.iter().any(|name| process.eq_ignore_ascii_case(name))
}

fn cursor_on_taskbar() -> bool {
    unsafe {
        let mut point: POINT = zeroed();
        if GetCursorPos(&mut point) == 0 {
            return false;
        }

        let root = GetAncestor(WindowFromPoint(point), GA_ROOT);
        if root.is_null() {
            return false;
        }

        let mut buffer = [0u16; 64];
        let length = GetClassNameW(root, buffer.as_mut_ptr(), buffer.len() as i32);
        if length <= 0 {
            return false;
        }

        let class = String::from_utf16_lossy(&buffer[..length as usize]);
        class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd"
    }
}
