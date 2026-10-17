use std::ptr;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

use crate::win::wide;

const CF_UNICODETEXT: u32 = 13;

/// Puts `text` on the clipboard. Another process can hold the clipboard open
/// for a moment, so opening is retried briefly before giving up — as
/// WinForms' `Clipboard.SetText` did.
pub fn set_text(owner: HWND, text: &str) -> Result<(), String> {
    let units = wide(text);

    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(owner) } != 0 {
            opened = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    if !opened {
        return Err("the clipboard is in use by another application".into());
    }

    let result = unsafe { place(&units) };
    unsafe { CloseClipboard() };
    result
}

/// Must be called with the clipboard open.
unsafe fn place(units: &[u16]) -> Result<(), String> {
    if EmptyClipboard() == 0 {
        return Err("the clipboard could not be emptied".into());
    }

    let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
    if memory.is_null() {
        return Err("out of memory for the clipboard".into());
    }

    let target = GlobalLock(memory) as *mut u16;
    if target.is_null() {
        GlobalFree(memory);
        return Err("clipboard memory could not be locked".into());
    }

    ptr::copy_nonoverlapping(units.as_ptr(), target, units.len());
    GlobalUnlock(memory);

    // On success the system owns the memory; on failure it is still ours.
    if SetClipboardData(CF_UNICODETEXT, memory).is_null() {
        GlobalFree(memory);
        return Err("the clipboard refused the text".into());
    }

    Ok(())
}
