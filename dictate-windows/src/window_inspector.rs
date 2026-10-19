//! Identifies the window that had focus when the hotkey went down. Captured at
//! press time and re-checked at delivery time: if focus moved in between, the
//! text goes to the clipboard rather than into whatever is in front now.

use std::path::Path;

use dictate_core::text_sanitizer;
use dictate_core::TargetContext;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HWND};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn describe(window: HWND, extra_console_processes: &[String]) -> TargetContext {
    if window.is_null() {
        return TargetContext::unknown();
    }

    let title = read_title(window);
    let process = process_name(window);
    let is_console =
        text_sanitizer::looks_like_console(&process, extra_console_processes) || is_console_window_class(window);

    TargetContext::new(process, title, is_console)
}

fn read_title(window: HWND) -> String {
    let mut buffer = [0u16; 512];
    let length = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length > 0 {
        String::from_utf16_lossy(&buffer[..length as usize])
    } else {
        String::new()
    }
}

/// Whether the window belongs to a process above dictate's integrity level —
/// in practice one running as administrator. Such a process refuses full query
/// access to an unelevated one; limited access still works, so a refusal of
/// the full kind is the signal (checked on this machine against an
/// administrator PowerShell).
pub fn is_elevated(window: HWND) -> bool {
    let mut pid = 0u32;

    unsafe {
        if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
            return false;
        }

        let process = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if process.is_null() {
            return GetLastError() == ERROR_ACCESS_DENIED;
        }

        CloseHandle(process);
        false
    }
}

/// Executable name without extension, e.g. "wezterm-gui".
/// Limited-information access works on elevated processes too.
pub fn process_name(window: HWND) -> String {
    let mut pid = 0u32;

    unsafe {
        if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
            return String::new();
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new(); // exited between the two calls
        }

        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let found = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) != 0;
        CloseHandle(process);

        if !found {
            return String::new();
        }

        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned()
    }
}

/// Catches consoles the process name misses — a console window hosted by
/// conhost still reports the class ConsoleWindowClass regardless of which
/// executable is running inside it.
fn is_console_window_class(window: HWND) -> bool {
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 0 {
        return false;
    }

    let class = String::from_utf16_lossy(&buffer[..length as usize]);
    class == "ConsoleWindowClass" || class == "PseudoConsoleWindow"
}
