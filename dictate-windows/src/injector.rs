//! Types text with SendInput using Unicode scan codes, so it works in terminals
//! and over RDP without depending on the target's paste chord — and without
//! touching the clipboard.

use std::mem::size_of;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC_EX,
};

/// Stamped into dwExtraInfo on everything we synthesise, so our own hook can
/// recognise our keystrokes and refuse to react to them.
pub const INJECTION_SIGNATURE: usize = 0x0D1C_7A7E;

/// Sends `text` as keystrokes, `chunk_size` UTF-16 units per SendInput call.
/// One call is fastest, but some applications drop events that arrive that
/// fast; `chunk_delay_ms` is for them.
pub fn type_text(text: &str, chunk_size: i32, chunk_delay_ms: i32) -> Result<(), String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let size = chunk_size.max(1) as usize;

    let mut chunks = units.chunks(size).peekable();
    while let Some(chunk) = chunks.next() {
        send_chunk(chunk)?;

        if chunk_delay_ms > 0 && chunks.peek().is_some() {
            thread::sleep(Duration::from_millis(chunk_delay_ms as u64));
        }
    }

    Ok(())
}

/// One tap of a virtual key, stamped like everything else we send. Returns
/// whether Windows accepted both events.
pub fn tap_key(virtual_key: u16) -> bool {
    let inputs = [virtual_key_event(virtual_key, false), virtual_key_event(virtual_key, true)];
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent == 1 {
        // A partial tap must not leave a modifier down. Retry only the up;
        // replaying the down would make the problem worse.
        unsafe { SendInput(1, &inputs[1], size_of::<INPUT>() as i32) };
    }
    sent as usize == inputs.len()
}

fn virtual_key_event(virtual_key: u16, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(u32::from(virtual_key), MAPVK_VK_TO_VSC_EX) };
    let extended = if scan & 0xFF00 == 0xE000 { KEYEVENTF_EXTENDEDKEY } else { 0 };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: 0,
                dwFlags: extended | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: INJECTION_SIGNATURE,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_key_taps_use_matching_extended_events() {
        for key in [0x5B, 0x5C] {
            let down = unsafe { virtual_key_event(key, false).Anonymous.ki };
            let up = unsafe { virtual_key_event(key, true).Anonymous.ki };
            assert_eq!(down.dwFlags, KEYEVENTF_EXTENDEDKEY);
            assert_eq!(up.dwFlags, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP);
            assert_eq!(down.wVk, up.wVk);
            assert_eq!(up.dwExtraInfo, INJECTION_SIGNATURE);
        }
        let caps = unsafe { virtual_key_event(0x14, true).Anonymous.ki };
        assert_eq!(caps.dwFlags, KEYEVENTF_KEYUP);
    }
}

fn send_chunk(chunk: &[u16]) -> Result<(), String> {
    // Two events per UTF-16 code unit: down then up. Code units rather than
    // chars on purpose — a surrogate pair must go as two events, which is
    // exactly what Windows expects.
    let inputs: Vec<INPUT> = chunk.iter().flat_map(|&unit| [key_event(unit, false), key_event(unit, true)]).collect();

    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        return Ok(());
    }

    // Most often UIPI: the foreground window belongs to an elevated process and
    // refuses input from this one. The text is shown to the user (FR-14.6);
    // the Win32 code rides along in brackets for the diagnostic log.
    let error = unsafe { GetLastError() };
    let what = if sent == 0 { "dictate could not type into this window" } else { "Only part of the text was typed" };
    Err(format!(
        "{what}. Windows blocks typing into a window that runs as administrator. [SendInput {sent}/{}, error {error}]",
        inputs.len()
    ))
}

fn key_event(unit: u16, up: bool) -> INPUT {
    let release = if up { KEYEVENTF_KEYUP } else { 0 };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE | release,
                time: 0,
                dwExtraInfo: INJECTION_SIGNATURE,
            },
        },
    }
}
