//! The notification-area icon, its balloon notifications and its menu.
//!
//! Icons are drawn at runtime, one per state, so the repository carries no
//! binary .ico files and the colours live next to the states they mean.

use std::mem::{size_of, zeroed};
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIcon, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, HICON, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

use crate::overlay::SessionState;
use crate::win::{fill, instance, wide, WM_TRAY};

const ICON_ID: u32 = 1;

const ID_OPEN_CONFIG: usize = 10;
const ID_REAUTH: usize = 11;
const ID_QUIT: usize = 12;
const ID_RECENT_BASE: usize = 100;

pub enum MenuChoice {
    Nothing,
    Recent(usize),
    OpenConfigFolder,
    ReEnterKeys,
    Quit,
}

pub struct Tray {
    hwnd: HWND,
    idle: HICON,
    recording: HICON,
    transcribing: HICON,
    state: SessionState,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let tray = Self {
            hwnd,
            idle: draw_icon(148, 163, 184),        // slate
            recording: draw_icon(239, 68, 68),     // red
            transcribing: draw_icon(59, 130, 246), // blue
            state: SessionState::Idle,
        };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = ICON_ID;
        data
    }

    fn icon_for(&self, state: SessionState) -> HICON {
        match state {
            SessionState::Idle => self.idle,
            SessionState::Recording => self.recording,
            SessionState::Transcribing => self.transcribing,
        }
    }

    /// Also called when Explorer restarts and the notification area is rebuilt
    /// without us.
    pub fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icon_for(self.state);
        fill(&mut data.szTip, &format!("dictate — {}", self.state.name()));
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    }

    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_TIP;
        data.hIcon = self.icon_for(state);
        fill(&mut data.szTip, &format!("dictate — {}", state.name()));
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn notify(&self, title: &str, detail: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        fill(&mut data.szInfoTitle, title);
        fill(&mut data.szInfo, detail);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = self.data();
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
            DestroyIcon(self.idle);
            DestroyIcon(self.recording);
            DestroyIcon(self.transcribing);
        }
    }
}

/// Shows the context menu at the cursor and returns what was picked. Runs a
/// modal loop, so the caller must not be holding application state.
pub fn show_menu(hwnd: HWND, recent: &[String]) -> MenuChoice {
    unsafe {
        let menu = CreatePopupMenu();
        let recent_menu = CreatePopupMenu();

        for (index, text) in recent.iter().enumerate() {
            let label = wide(&menu_label(text));
            AppendMenuW(recent_menu, MF_STRING, ID_RECENT_BASE + index, label.as_ptr());
        }

        let recent_flags = if recent.is_empty() { MF_POPUP | MF_GRAYED } else { MF_POPUP };
        let recent_label = wide("Recent");
        AppendMenuW(menu, recent_flags, recent_menu as usize, recent_label.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let open_config = wide("Open config folder");
        AppendMenuW(menu, MF_STRING, ID_OPEN_CONFIG, open_config.as_ptr());
        let reauth = wide("Re-enter API keys…");
        AppendMenuW(menu, MF_STRING, ID_REAUTH, reauth.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        let quit = wide("Quit");
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());

        let mut cursor: POINT = zeroed();
        GetCursorPos(&mut cursor);

        // Both documented requirements for a tray menu that closes when the
        // user clicks elsewhere. Taking the foreground here is fine: the menu
        // is not a dictation, and nothing is pinned while it is open.
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            0,
            hwnd,
            null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);

        DestroyMenu(menu); // destroys the submenu with it

        match command as usize {
            0 => MenuChoice::Nothing,
            ID_OPEN_CONFIG => MenuChoice::OpenConfigFolder,
            ID_REAUTH => MenuChoice::ReEnterKeys,
            ID_QUIT => MenuChoice::Quit,
            id if id >= ID_RECENT_BASE => MenuChoice::Recent(id - ID_RECENT_BASE),
            _ => MenuChoice::Nothing,
        }
    }
}

/// First 60 characters, one line, with `&` doubled so it is not read as a
/// mnemonic marker.
fn menu_label(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c == '\r' || c == '\n' { ' ' } else { c }).collect();
    let mut label: String = flat.chars().take(60).collect();
    if flat.chars().count() > 60 {
        label.push('…');
    }
    label.replace('&', "&&")
}

/// A filled circle with a soft white rim, 32×32 BGRA with straight alpha,
/// anti-aliased by coverage.
fn draw_icon(r: u8, g: u8, b: u8) -> HICON {
    const SIZE: usize = 32;
    const RADIUS: f32 = 12.0;

    let mut colour = vec![0u8; SIZE * SIZE * 4];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - 16.0;
            let dy = y as f32 + 0.5 - 16.0;
            let distance = (dx * dx + dy * dy).sqrt();

            let fill = (RADIUS + 0.5 - distance).clamp(0.0, 1.0);
            let rim = (1.5 - (distance - RADIUS).abs()).clamp(0.0, 1.0) * (200.0 / 255.0);
            let alpha = rim + fill * (1.0 - rim);

            let blend = |channel: u8| -> u8 {
                if alpha <= 0.0 {
                    0
                } else {
                    ((255.0 * rim + f32::from(channel) * fill * (1.0 - rim)) / alpha).round() as u8
                }
            };

            let i = (y * SIZE + x) * 4;
            colour[i] = blend(b);
            colour[i + 1] = blend(g);
            colour[i + 2] = blend(r);
            colour[i + 3] = (alpha * 255.0).round() as u8;
        }
    }

    // With 32-bit colour the alpha channel decides transparency; an all-zero
    // mask just says "use it".
    let mask = vec![0u8; SIZE * SIZE / 8];

    unsafe { CreateIcon(instance(), SIZE as i32, SIZE as i32, 1, 32, mask.as_ptr(), colour.as_ptr()) }
}
