//! A small always-on-top pill showing what dictate is doing.
//!
//! The one non-negotiable property is that it never takes focus (FR-14.4): the
//! pinned-target contract records the foreground window at hotkey-down, and an
//! overlay that activated itself would break that on every utterance.
//! WS_EX_NOACTIVATE, SWP_NOACTIVATE and MA_NOACTIVATE are what guarantee it.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use dictate_core::OverlayPosition;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW,
    InvalidateRect, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    HFONT, LOGFONTW, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos, RegisterClassExW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, SystemParametersInfoW, HWND_TOPMOST, LWA_ALPHA,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::win::{instance, wide};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Recording,
    Transcribing,
}

impl SessionState {
    pub fn name(self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Recording => "recording",
            SessionState::Transcribing => "transcribing",
        }
    }
}

const SPI_GETWORKAREA: u32 = 0x0030;
const MA_NOACTIVATE: LRESULT = 3;

struct View {
    text: Vec<u16>,
    colour: u32,
    font: HFONT,
}

thread_local! {
    static VIEW: RefCell<View> = const { RefCell::new(View { text: Vec::new(), colour: 0, font: null_mut() }) };
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

pub struct Overlay {
    hwnd: HWND,
    font: HFONT,
    position: OverlayPosition,
    width: i32,
    height: i32,
    dpi: i32,
}

impl Overlay {
    pub fn new(position: OverlayPosition) -> Result<Self, String> {
        let class = wide("dictate.overlay");

        unsafe {
            let mut wc: WNDCLASSEXW = zeroed();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(overlay_proc);
            wc.hInstance = instance();
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);

            // Created now, not mid-utterance: the first window creation costs
            // tens of milliseconds the user would otherwise wait for.
            let title = wide("");
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                190,
                44,
                null_mut(),
                null_mut(),
                instance(),
                null(),
            );
            if hwnd.is_null() {
                return Err("Could not create the recording indicator.".into());
            }

            SetLayeredWindowAttributes(hwnd, 0, 235, LWA_ALPHA);

            let dpi = match GetDpiForWindow(hwnd) {
                0 => 96,
                dpi => dpi as i32,
            };

            let mut font: LOGFONTW = zeroed();
            font.lfHeight = -(10 * dpi + 36) / 72;
            font.lfWeight = 400;
            font.lfCharSet = 1; // DEFAULT_CHARSET
            font.lfQuality = 5; // CLEARTYPE_QUALITY
            crate::win::fill(&mut font.lfFaceName, "Segoe UI");
            let font = CreateFontIndirectW(&font);

            VIEW.with(|view| view.borrow_mut().font = font);

            Ok(Self { hwnd, font, position, width: 190 * dpi / 96, height: 44 * dpi / 96, dpi })
        }
    }

    pub fn show_state(&self, state: SessionState) {
        let (text, colour) = match state {
            SessionState::Idle => {
                unsafe { ShowWindow(self.hwnd, SW_HIDE) };
                return;
            }
            SessionState::Recording => ("● recording", rgb(248, 113, 113)),
            SessionState::Transcribing => ("… transcribing", rgb(147, 197, 253)),
        };

        VIEW.with(|view| {
            let mut view = view.borrow_mut();
            view.text = text.encode_utf16().collect();
            view.colour = colour;
        });

        let (x, y) = self.place();

        // HWND_TOPMOST on every appearance: another application going topmost
        // while dictate was idle can otherwise leave the indicator behind it,
        // which is exactly when it is needed.
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, self.width, self.height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            InvalidateRect(self.hwnd, null(), 1);
        }
    }

    /// Corner positions use the **primary** screen's working area, so the pill
    /// sits above the taskbar and stays put across a multi-monitor desktop.
    fn place(&self) -> (i32, i32) {
        let margin = 16 * self.dpi / 96;
        let gap = 8 * self.dpi / 96;

        unsafe {
            if self.position == OverlayPosition::NearCursor {
                let mut cursor: POINT = zeroed();
                GetCursorPos(&mut cursor);

                let mut info: MONITORINFO = zeroed();
                info.cbSize = size_of::<MONITORINFO>() as u32;
                GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info);
                let work = info.rcWork;

                let x = (cursor.x + 20 * self.dpi / 96).min(work.right - self.width - gap);
                let y = (cursor.y + 28 * self.dpi / 96).min(work.bottom - self.height - gap);
                return (x.max(work.left + gap), y.max(work.top + gap));
            }

            let mut work: RECT = zeroed();
            SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut c_void, 0);

            let right = matches!(self.position, OverlayPosition::BottomRight | OverlayPosition::TopRight);
            let bottom = matches!(self.position, OverlayPosition::BottomRight | OverlayPosition::BottomLeft);

            let x = if right { work.right - self.width - margin } else { work.left + margin };
            let y = if bottom { work.bottom - self.height - margin } else { work.top + margin };
            (x, y)
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            DeleteObject(self.font);
        }
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => MA_NOACTIVATE,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);

            let mut area: RECT = zeroed();
            GetClientRect(hwnd, &mut area);

            let background = CreateSolidBrush(rgb(24, 24, 27));
            FillRect(hdc, &area, background);
            DeleteObject(background);

            VIEW.with(|view| {
                let view = view.borrow();
                let previous = SelectObject(hdc, view.font);
                SetBkMode(hdc, 1); // TRANSPARENT
                SetTextColor(hdc, view.colour);

                let mut text = view.text.clone();
                DrawTextW(hdc, text.as_mut_ptr(), text.len() as i32, &mut area, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

                SelectObject(hdc, previous);
            });

            EndPaint(hwnd, &paint);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}
