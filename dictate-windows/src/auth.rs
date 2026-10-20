//! Collects the two API keys and stores them in Credential Manager.
//!
//! A window rather than a console prompt because this ships as a GUI
//! executable: it has no console to read from.

use std::cell::Cell;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetSysColor, GetSysColorBrush, SetBkMode, SetTextColor, HDC, LOGFONTW,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgCtrlID, GetDlgItem,
    GetMessageW, GetSystemMetrics, GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, LoadCursorW,
    PostQuitMessage, RegisterClassExW, SendMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, IDC_ARROW,
    MB_ICONERROR, MB_ICONWARNING, MSG, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_SETFONT, WNDCLASSEXW,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};

use crate::credentials;
use crate::win::{instance, loword, message_box, wide};

const ID_SAVE: i32 = 1; // IDOK: what Enter sends
const ID_CANCEL: i32 = 2; // IDCANCEL: what Escape sends
const ID_ELEVENLABS: i32 = 101;
const ID_ANTHROPIC: i32 = 102;
const ID_NOTE: i32 = 103;

const ES_PASSWORD: u32 = 0x0020;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const COLOR_BTNFACE: i32 = 15;
const COLOR_GRAYTEXT: i32 = 17;

thread_local! {
    static OUTCOME: Cell<Option<bool>> = const { Cell::new(None) };
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Shows the dialog and runs its loop. Returns true if keys were saved.
pub fn prompt() -> bool {
    // The tray menu stays live while this is open; a second copy would share
    // the outcome slot.
    if OPEN.with(|open| open.replace(true)) {
        return false;
    }

    let saved = unsafe { run() };
    OPEN.with(|open| open.set(false));
    saved
}

unsafe fn run() -> bool {
    OUTCOME.with(|outcome| outcome.set(None));

    let dpi = match GetDpiForSystem() {
        0 => 96,
        dpi => dpi as i32,
    };
    let scale = |value: i32| value * dpi / 96;

    let class = wide("dictate.auth");
    let mut wc: WNDCLASSEXW = zeroed();
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(auth_proc);
    wc.hInstance = instance();
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = GetSysColorBrush(COLOR_BTNFACE as _);
    wc.lpszClassName = class.as_ptr();
    RegisterClassExW(&wc);

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let ex_style = WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT;
    let mut frame = RECT { left: 0, top: 0, right: scale(460), bottom: scale(210) };
    AdjustWindowRectEx(&mut frame, style, 0, ex_style);
    let width = frame.right - frame.left;
    let height = frame.bottom - frame.top;
    let x = (GetSystemMetrics(0) - width) / 2; // SM_CXSCREEN
    let y = (GetSystemMetrics(1) - height) / 2; // SM_CYSCREEN

    let title = wide("dictate — API keys");
    let hwnd = CreateWindowExW(
        ex_style,
        class.as_ptr(),
        title.as_ptr(),
        style,
        x,
        y,
        width,
        height,
        null_mut(),
        null_mut(),
        instance(),
        null(),
    );
    if hwnd.is_null() {
        message_box(null_mut(), "Could not open the API key dialog.", MB_ICONERROR);
        return false;
    }

    let mut font: LOGFONTW = zeroed();
    font.lfHeight = -(9 * dpi + 36) / 72;
    font.lfWeight = 400;
    font.lfCharSet = 1;
    font.lfQuality = 5;
    crate::win::fill(&mut font.lfFaceName, "Segoe UI");
    let font = CreateFontIndirectW(&font);

    let control = |class: &str, text: &str, style: u32, ex: u32, x: i32, y: i32, w: i32, h: i32, id: i32| {
        let class = wide(class);
        let text = wide(text);
        let child = CreateWindowExW(
            ex,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            scale(x),
            scale(y),
            scale(w),
            scale(h),
            hwnd,
            id as isize as _,
            instance(),
            null(),
        );
        SendMessageW(child, WM_SETFONT, font as usize, 1);
        child
    };

    let existing_elevenlabs = credentials::read(credentials::ELEVENLABS).unwrap_or_default();
    let existing_anthropic = credentials::read(credentials::ANTHROPIC).unwrap_or_default();

    control(
        "STATIC",
        "Keys are stored in Windows Credential Manager, encrypted under your\nWindows account. They are never written to a file.",
        0, 0, 12, 12, 436, 34, 0,
    );
    control("STATIC", "ElevenLabs key", 0, 0, 12, 58, 120, 20, 0);
    let first = control(
        "EDIT",
        &existing_elevenlabs,
        WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        140, 55, 300, 23, ID_ELEVENLABS,
    );
    control("STATIC", "Anthropic key", 0, 0, 12, 92, 120, 20, 0);
    control(
        "EDIT",
        &existing_anthropic,
        WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL,
        WS_EX_CLIENTEDGE,
        140, 89, 300, 23, ID_ANTHROPIC,
    );
    control("STATIC", "Leave the Anthropic key blank to dictate without cleanup.", 0, 0, 140, 116, 300, 20, ID_NOTE);
    control("BUTTON", "Save", WS_TABSTOP | BS_DEFPUSHBUTTON, 0, 264, 158, 85, 28, ID_SAVE);
    control("BUTTON", "Cancel", WS_TABSTOP, 0, 355, 158, 85, 28, ID_CANCEL);

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(first);

    let mut message: MSG = zeroed();
    let saved = loop {
        if let Some(saved) = OUTCOME.with(|outcome| outcome.get()) {
            break saved;
        }

        let got = GetMessageW(&mut message, null_mut(), 0, 0);
        if got == 0 {
            // WM_QUIT belongs to the outer loop; put it back for it.
            PostQuitMessage(message.wParam as i32);
            break false;
        }
        if got < 0 {
            break false;
        }

        // Tab, Enter and Escape behave as in a dialog.
        if IsDialogMessageW(hwnd, &message) == 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    DestroyWindow(hwnd);
    DeleteObject(font as _);
    saved
}

unsafe fn text_of(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    if length <= 0 {
        return String::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

unsafe fn save(hwnd: HWND) {
    let elevenlabs = text_of(GetDlgItem(hwnd, ID_ELEVENLABS)).trim().to_owned();

    if elevenlabs.is_empty() {
        message_box(
            hwnd,
            "An ElevenLabs key is required — without it there is nothing to transcribe with.",
            MB_ICONWARNING,
        );
        return;
    }

    let anthropic = text_of(GetDlgItem(hwnd, ID_ANTHROPIC)).trim().to_owned();

    let stored = credentials::write(credentials::ELEVENLABS, &elevenlabs).and_then(|()| {
        if anthropic.is_empty() {
            credentials::delete(credentials::ANTHROPIC);
            Ok(())
        } else {
            credentials::write(credentials::ANTHROPIC, &anthropic)
        }
    });

    match stored {
        Ok(()) => OUTCOME.with(|outcome| outcome.set(Some(true))),
        Err(error) => message_box(hwnd, &error, MB_ICONERROR),
    }
}

unsafe extern "system" fn auth_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_COMMAND => {
            match i32::from(loword(wparam)) {
                ID_SAVE => save(hwnd),
                ID_CANCEL => OUTCOME.with(|outcome| outcome.set(Some(false))),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            OUTCOME.with(|outcome| outcome.set(Some(false)));
            0
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wparam as HDC;
            SetBkMode(hdc, 1); // TRANSPARENT
            if GetDlgCtrlID(lparam as HWND) == ID_NOTE {
                SetTextColor(hdc, GetSysColor(COLOR_GRAYTEXT as _));
            }
            GetSysColorBrush(COLOR_BTNFACE as _) as LRESULT
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
