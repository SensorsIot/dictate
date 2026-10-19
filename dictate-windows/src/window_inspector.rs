//! Identifies the window that had focus when the hotkey went down. Captured at
//! press time and re-checked at delivery time: if focus moved in between, the
//! text goes to the clipboard rather than into whatever is in front now.

use std::path::Path;

use dictate_core::text_sanitizer;
use dictate_core::TargetContext;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
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
    let process = read_process_name(window);
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

/// Executable name without extension, like .NET's `Process.ProcessName`.
/// Limited-information access works on elevated processes too.
fn read_process_name(window: HWND) -> String {
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
