//! Small helpers shared by every module that talks to Win32.

use std::ptr::null;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, PostMessageW, MB_OK, WM_APP};

// Private messages to the main window. Everything that happens off the UI
// thread, or inside the keyboard hook, arrives as one of these.
pub const WM_HOTKEY_DOWN: u32 = WM_APP + 1;
pub const WM_HOTKEY_UP: u32 = WM_APP + 2;
pub const WM_HOTKEY_REJECTED: u32 = WM_APP + 3;
pub const WM_RECORDER_LIMIT: u32 = WM_APP + 4;
pub const WM_RECORDER_FIRST_BUFFER: u32 = WM_APP + 5;
pub const WM_RECORDER_CLOSED: u32 = WM_APP + 6;
pub const WM_PIPELINE_DONE: u32 = WM_APP + 7;
pub const WM_TRAY: u32 = WM_APP + 8;

/// NUL-terminated UTF-16, for every `PCWSTR` parameter.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `text` into a fixed-size UTF-16 field, truncating and terminating.
pub fn fill(field: &mut [u16], text: &str) {
    if field.is_empty() {
        return;
    }

    let mut length = 0;
    for unit in text.encode_utf16().take(field.len() - 1) {
        field[length] = unit;
        length += 1;
    }
    field[length] = 0;
}

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(null()) }
}

pub fn message_box(owner: HWND, text: &str, icon: u32) {
    let text = wide(text);
    let caption = wide("dictate");
    unsafe {
        MessageBoxW(owner, text.as_ptr(), caption.as_ptr(), MB_OK | icon);
    }
}

pub fn loword(value: usize) -> u16 {
    (value & 0xFFFF) as u16
}

/// A window handle that may cross threads. Only ever used to post messages,
/// which Win32 permits from any thread.
#[derive(Clone, Copy)]
pub struct SendHwnd(isize);

impl SendHwnd {
    pub fn new(hwnd: HWND) -> Self {
        Self(hwnd as isize)
    }

    pub fn post(self, message: u32, wparam: usize, lparam: isize) -> bool {
        unsafe { PostMessageW(self.0 as HWND, message, wparam, lparam) != 0 }
    }
}
