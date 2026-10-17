//! Types text with SendInput using Unicode scan codes, so it works in terminals
//! and over RDP without depending on the target's paste chord — and without
//! touching the clipboard.

use std::mem::size_of;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
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
    // refuses input from this one.
    let error = unsafe { GetLastError() };
    Err(format!(
        "SendInput delivered {sent} of {} events (Win32 error {error}). \
         If the focused window is running as administrator, dictate cannot type into it.",
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
