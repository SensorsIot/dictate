//! Types text as Quartz keyboard events carrying Unicode strings, so it works in
//! terminals and editors alike without touching the clipboard.

use std::thread;
use std::time::Duration;

use crate::ffi::*;

/// Stamped into kCGEventSourceUserData on everything we post, so our own tap
/// can recognise our keystrokes and refuse to react to them.
pub const INJECTION_SIGNATURE: i64 = 0x0D1C_7A7E;

/// The most UTF-16 units one event carries; CGEventKeyboardSetUnicodeString
/// silently truncates beyond it.
const UNITS_PER_EVENT: usize = 20;

const KEYCODE_RETURN: CGKeyCode = 36;

/// Sends `text` as keystrokes, pausing `chunk_delay_ms` after every
/// `chunk_size` UTF-16 units for applications that drop fast input.
pub fn type_text(text: &str, chunk_size: i32, chunk_delay_ms: i32) -> Result<(), String> {
    // Without Accessibility the system drops posted events without a word,
    // which would look like a successful delivery of nothing.
    if !unsafe { AXIsProcessTrusted() } {
        return Err("dictate is not allowed to type. System Settings → Privacy & Security → Accessibility: \
                    switch on dictate."
            .into());
    }

    let chunk_size = chunk_size.max(1) as usize;
    let mut since_pause = 0;

    // A line break is the Return key: most applications ignore a newline
    // character in a Unicode key event. Consoles never get here with one
    // (FR-6.4).
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            post_key(KEYCODE_RETURN, &[]);
        }

        let units: Vec<u16> = line.trim_end_matches('\r').encode_utf16().collect();
        let mut rest = units.as_slice();
        while !rest.is_empty() {
            let mut take = rest.len().min(UNITS_PER_EVENT);
            // Never split a surrogate pair across two events.
            if take < rest.len() && (0xD800..0xDC00).contains(&rest[take - 1]) {
                take -= 1;
            }

            post_key(0, &rest[..take]);
            rest = &rest[take..];

            since_pause += take;
            if chunk_delay_ms > 0 && since_pause >= chunk_size {
                since_pause = 0;
                thread::sleep(Duration::from_millis(chunk_delay_ms as u64));
            }
        }
    }

    Ok(())
}

/// One down/up pair. With `units`, the pair types that string whatever the
/// key code; without, it is the key itself.
fn post_key(keycode: CGKeyCode, units: &[u16]) {
    for down in [true, false] {
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, down);
            if event.is_null() {
                continue;
            }

            // No modifiers: a Command still held at delivery would otherwise
            // turn the text into shortcuts.
            CGEventSetFlags(event, 0);
            CGEventSetIntegerValueField(event, kCGEventSourceUserData, INJECTION_SIGNATURE);
            if !units.is_empty() {
                CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
            }

            CGEventPost(kCGHIDEventTap, event);
            CFRelease(event);
        }
    }
}
