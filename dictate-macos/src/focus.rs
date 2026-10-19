//! Which window has focus, and what it is.
//!
//! The window list is ordered front to back, and the first ordinary window
//! (layer 0) on screen is the one the user is typing into. Its number is what
//! FR-6.1 pins: a second window of the same application is a different target.
//! Owner and number need no permission; only window titles would.

use dictate_core::{text_sanitizer, TargetContext};

use crate::ffi::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub pid: i64,
    pub number: i64,
}

impl Window {
    const NONE: Window = Window { pid: 0, number: 0 };
}

pub fn foreground() -> Window {
    front_window().map(|(window, _)| window).unwrap_or(Window::NONE)
}

/// FR-13.2: a window gone by the time it is described yields an empty
/// descriptor.
pub fn describe(window: Window, extra_console_processes: &[String]) -> TargetContext {
    if window == Window::NONE {
        return TargetContext::unknown();
    }

    let owner = window_list()
        .into_iter()
        .find(|(candidate, _)| *candidate == window)
        .map(|(_, owner)| owner)
        .unwrap_or_default();
    let title = focused_window_title(window.pid).unwrap_or_default();
    let is_console = text_sanitizer::looks_like_console(&owner, extra_console_processes);

    TargetContext::new(owner, title, is_console)
}

fn front_window() -> Option<(Window, String)> {
    window_list().into_iter().next()
}

/// On-screen layer-0 windows, front to back, with their owner's name.
fn window_list() -> Vec<(Window, String)> {
    let mut windows = Vec::new();

    unsafe {
        let list = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, 0);
        if list.is_null() {
            return windows;
        }

        for index in 0..CFArrayGetCount(list) {
            let info = CFArrayGetValueAtIndex(list, index);
            if number(CFDictionaryGetValue(info, kCGWindowLayer)) != Some(0) {
                continue;
            }

            let (Some(pid), Some(window_number)) =
                (number(CFDictionaryGetValue(info, kCGWindowOwnerPID)), number(CFDictionaryGetValue(info, kCGWindowNumber)))
            else {
                continue;
            };
            let owner = rust_string(CFDictionaryGetValue(info, kCGWindowOwnerName)).unwrap_or_default();

            windows.push((Window { pid, number: window_number }, owner));
        }

        CFRelease(list);
    }

    windows
}

/// The title of the application's focused window, through Accessibility.
/// None without that permission, which costs the cleanup prompt some context
/// and nothing else.
fn focused_window_title(pid: i64) -> Option<String> {
    unsafe {
        let application = AXUIElementCreateApplication(pid as i32);
        if application.is_null() {
            return None;
        }

        let focused_attribute = cf_string("AXFocusedWindow");
        let title_attribute = cf_string("AXTitle");

        let mut window: CFTypeRef = std::ptr::null();
        let mut title = None;
        if AXUIElementCopyAttributeValue(application, focused_attribute, &mut window) == kAXErrorSuccess
            && !window.is_null()
        {
            let mut value: CFTypeRef = std::ptr::null();
            if AXUIElementCopyAttributeValue(window, title_attribute, &mut value) == kAXErrorSuccess && !value.is_null()
            {
                title = rust_string(value);
                CFRelease(value);
            }
            CFRelease(window);
        }

        CFRelease(focused_attribute);
        CFRelease(title_attribute);
        CFRelease(application);
        title
    }
}
