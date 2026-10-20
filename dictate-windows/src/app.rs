//! The tray application: owns the session state machine of FSD §5 and wires
//! the hotkey, the recorder, the pipeline and delivery together.
//!
//! Everything interactive runs on one thread and one message loop. The hook,
//! the recorder's capture thread and the pipeline worker never touch the state
//! below directly; they post messages to the main window, and the work happens
//! on the next pump.
//!
//! The state lives in a thread-local `RefCell`. Anything that runs a modal loop
//! — the tray menu, the key dialog, a message box — re-enters the window
//! procedure, so those are only ever called with the state *not* borrowed.

use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::panic::{self, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use dictate_core::{
    field, DiagnosticLog, DictateConfig, DictationPipeline, Platform, Session, SessionState, StopReason,
    TargetContext,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, PostQuitMessage,
    RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage, MB_ICONWARNING, MSG, SW_SHOWNORMAL,
    WM_CONTEXTMENU, WM_DESTROY, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPED,
};

use crate::audio::{self, resolve_input, Recorder, RecorderEvent};
use crate::overlay::Overlay;
use crate::tray::{self, MenuChoice, Tray};
use crate::win::{
    instance, message_box, wide, SendHwnd, WM_HOTKEY_DOWN, WM_HOTKEY_REJECTED, WM_HOTKEY_UP, WM_PIPELINE_DONE,
    WM_RECORDER_CLOSED, WM_RECORDER_FIRST_BUFFER, WM_RECORDER_LIMIT, WM_RECORDER_OPEN_FAILED, WM_START_FROM_ELEVATED,
    WM_TRAY,
};
use crate::{auth, autostart, clipboard, hotkey, injector, paths, start_guard, window_inspector};

const TIMER_RECONCILE: usize = 1;
const TIMER_START_RECHECK: usize = 2;

/// Long enough for the Start menu's opening animation to have finished.
const START_RECHECK_MS: u32 = 400;

const VK_ESCAPE: u16 = 0x1B;

/// The Windows half of a session: what `Session` asks the operating system for.
struct WindowsPlatform {
    hwnd: HWND,
    recorder: Recorder,
    overlay: Overlay,
    tray: Tray,
    show_overlay: bool,
}

struct App {
    session: Session<WindowsPlatform>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut app| app.as_mut().map(f)))
}

pub fn run(config: DictateConfig, pipeline: DictationPipeline, log: DiagnosticLog) -> Result<(), String> {
    let hwnd = create_main_window()?;
    let log = Arc::new(log);

    let notify_window = SendHwnd::new(hwnd);
    let recorder = Recorder::new(
        config.maximum_recording_seconds,
        config.keep_microphone_open,
        config.input_device.clone(),
        Arc::new(move |event: RecorderEvent| {
            // Raised on the capture thread. Everything real is posted to the
            // UI thread, where the state machine lives.
            match event {
                RecorderEvent::LimitReached => notify_window.post(WM_RECORDER_LIMIT, 0, 0),
                RecorderEvent::FirstBuffer(after) => {
                    notify_window.post(WM_RECORDER_FIRST_BUFFER, after.as_millis() as usize, 0)
                }
                RecorderEvent::DeviceClosed(confirmed) => notify_window.post(WM_RECORDER_CLOSED, confirmed as usize, 0),
                RecorderEvent::OpenFailed => notify_window.post(WM_RECORDER_OPEN_FAILED, 0, 0),
            };
        }),
    );

    let overlay = Overlay::new(config.overlay_position).map_err(|e| {
        unsafe { DestroyWindow(hwnd) };
        e
    })?;
    let tray = Tray::new(hwnd);

    // Pays first-use costs at startup rather than on the first press.
    recorder.pre_warm();
    let _ = window_inspector::describe(window_inspector::foreground(), &[]);

    let started_fields = [
        field("version", env!("CARGO_PKG_VERSION")),
        field("hotkey", format!("0x{:02X}", config.hotkey_virtual_key)),
        field("keepMicOpen", config.keep_microphone_open),
        field("input", resolve_input(config.input_device.as_deref()).1),
        field("ignoreInjected", config.ignore_injected_hotkey),
        field("cleanup", format!("{} {}", config.cleanup_provider, config.effective_cleanup_model())),
    ];

    let hotkey_key = config.hotkey_virtual_key;
    let suppress = config.suppress_hotkey;
    let ignore_injected = config.ignore_injected_hotkey;

    let show_overlay = config.show_overlay;
    let platform = WindowsPlatform { hwnd, recorder, overlay, tray, show_overlay };
    let session = Session::new(platform, config, pipeline, Arc::clone(&log));

    APP.with(|cell| {
        *cell.borrow_mut() = Some(App { session });
    });

    // Installed last: once the hook is live the callbacks can arrive, and they
    // assume everything above exists.
    let graces = match hotkey::install(hotkey_key, suppress, ignore_injected, hwnd) {
        Ok(graces) => graces,
        Err(error) => {
            teardown(hwnd);
            return Err(error);
        }
    };

    unsafe { SetTimer(hwnd, TIMER_RECONCILE, hotkey::RECONCILE_INTERVAL_MS, None) };
    start_guard::install(hotkey_key, hwnd);

    let mut fields: Vec<_> = started_fields.into_iter().collect();
    fields.push(field("graceInitial", graces.initial));
    fields.push(field("graceRepeat", graces.repeat));
    log.event("started", &fields);

    unsafe {
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    teardown(hwnd);
    Ok(())
}

fn teardown(hwnd: HWND) {
    start_guard::uninstall();
    hotkey::uninstall();
    unsafe {
        KillTimer(hwnd, TIMER_RECONCILE);
        KillTimer(hwnd, TIMER_START_RECHECK);
    }

    // Dropping App closes the microphone (awaited), removes the tray icon and
    // destroys the overlay. FR-17.2: the recent list goes with it.
    let app = APP.with(|cell| cell.borrow_mut().take());
    drop(app);

    unsafe {
        DestroyWindow(hwnd);
    }
}

fn create_main_window() -> Result<HWND, String> {
    let class = wide("dictate.main");
    let title = wide("dictate");

    unsafe {
        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(main_proc);
        wc.hInstance = instance();
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc);

        // Never shown. A real top-level window rather than a message-only one,
        // because TrackPopupMenu needs a window that can be foreground.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance(),
            null(),
        );
        if hwnd.is_null() {
            return Err("Could not create the main window.".into());
        }

        let taskbar_created = wide("TaskbarCreated");
        TASKBAR_CREATED.with(|id| id.set(RegisterWindowMessageW(taskbar_created.as_ptr())));

        Ok(hwnd)
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A tray application that dies on a bug leaves the user with no dictation,
    // usually mid-sentence. Say what happened and keep the loop alive; the
    // state machine returns to Idle on the next press either way.
    match panic::catch_unwind(AssertUnwindSafe(|| handle(hwnd, message, wparam, lparam))) {
        Ok(Some(result)) => result,
        Ok(None) => DefWindowProcW(hwnd, message, wparam, lparam),
        Err(payload) => {
            let detail = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("unknown error");
            message_box(
                null_mut(),
                &format!("dictate hit an unexpected error and is still running:\n\n{detail}"),
                MB_ICONWARNING,
            );
            0
        }
    }
}

fn handle(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if message == TASKBAR_CREATED.with(Cell::get) && message != 0 {
        with_app(|app| app.session.platform.tray.add());
        return Some(0);
    }

    match message {
        WM_HOTKEY_DOWN => {
            let origin = vec![
                field("injected", wparam & hotkey::ORIGIN_INJECTED != 0),
                field("lowerIL", wparam & hotkey::ORIGIN_LOWER_INTEGRITY != 0),
            ];
            let pressed_at = hotkey::pressed_at().unwrap_or_else(Instant::now);
            with_app(|app| app.session.begin(pressed_at, origin));
        }
        WM_HOTKEY_UP => {
            let reason = if wparam == hotkey::RELEASE_RECONCILED { StopReason::Reconciled } else { StopReason::Released };
            with_app(|app| app.session.end(reason));
        }
        WM_HOTKEY_REJECTED => {
            with_app(|app| {
                app.session.log.event(
                    "hotkey.rejected",
                    &[field("reason", "injected"), field("lowerIL", wparam & hotkey::ORIGIN_LOWER_INTEGRITY != 0)],
                )
            });
        }
        WM_RECORDER_LIMIT => {
            with_app(|app| app.session.end(StopReason::Limit));
        }
        WM_RECORDER_FIRST_BUFFER => {
            with_app(|app| app.session.log.event("mic.firstBuffer", &[field("after", Duration::from_millis(wparam as u64))]));
        }
        WM_RECORDER_CLOSED => {
            with_app(|app| app.session.log.event("mic.close", &[field("confirmed", wparam != 0)]));
        }
        WM_RECORDER_OPEN_FAILED => {
            with_app(|app| app.session.capture_failed());
        }
        WM_PIPELINE_DONE => {
            with_app(|app| app.session.finish());
        }
        WM_TIMER if wparam == TIMER_RECONCILE => {
            hotkey::reconcile();
        }
        WM_START_FROM_ELEVATED => {
            // FR-8.8: Start is a lower-integrity window, so Esc reaches it.
            let closed = injector::tap_key(VK_ESCAPE);
            unsafe { SetTimer(hwnd, TIMER_START_RECHECK, START_RECHECK_MS, None) };
            with_app(|app| {
                app.session.log.event("start.closed", &[field("ok", closed)]);
                app.session.platform.tray.notify(
                    "Administrator window",
                    "Windows hides the hotkey from dictate in windows running as administrator. Dictate in an ordinary window and paste with Ctrl+V.",
                );
            });
        }
        WM_TIMER if wparam == TIMER_START_RECHECK => {
            unsafe { KillTimer(hwnd, TIMER_START_RECHECK) };
            // An Esc sent mid-animation can be lost. Only retried while Start
            // is still in front, so it never lands in an application.
            if start_guard::start_in_front() {
                let closed = injector::tap_key(VK_ESCAPE);
                with_app(|app| app.session.log.event("start.closeRetried", &[field("ok", closed)]));
            }
        }
        WM_TRAY => {
            let mouse = (lparam as usize & 0xFFFF) as u32;
            if mouse == WM_RBUTTONUP || mouse == WM_CONTEXTMENU {
                on_tray_menu(hwnd);
            }
        }
        WM_DESTROY => unsafe { PostQuitMessage(0) },
        _ => return None,
    }

    Some(0)
}

/// Runs the menu's modal loop with the application state released.
fn on_tray_menu(hwnd: HWND) {
    let (recent, configured): (Vec<String>, Option<String>) = with_app(|app| {
        (app.session.recent.items().into_iter().map(|u| u.text).collect(), app.session.config.input_device.clone())
    })
    .unwrap_or_default();

    let inputs = audio::list_inputs();
    let selected = configured.as_deref().and_then(|wanted| audio::find_input(&inputs, wanted));

    let autostart_on = autostart::is_enabled();

    match tray::show_menu(hwnd, &recent, &inputs, selected, autostart_on) {
        MenuChoice::Nothing => {}
        MenuChoice::ToggleAutostart => {
            let wanted = !autostart_on;
            let result = autostart::set_enabled(wanted);
            with_app(|app| {
                app.session.log.event("autostart", &[field("enabled", wanted), field("ok", result.is_ok())]);
                if let Err(error) = &result {
                    app.session.platform.tray.notify("Start with Windows", error);
                }
            });
        }
        MenuChoice::Microphone(choice) => {
            let name = choice.and_then(|index| inputs.get(index).cloned());
            with_app(|app| app.select_microphone(name));
        }
        MenuChoice::Recent(index) => {
            if let Some(text) = recent.get(index) {
                if let Err(error) = clipboard::set_text(hwnd, text) {
                    with_app(|app| app.session.platform.tray.notify("Could not copy", &error));
                }
            }
        }
        MenuChoice::OpenConfigFolder => open_config_folder(),
        MenuChoice::ReEnterKeys => {
            if auth::prompt() {
                with_app(|app| app.session.platform.tray.notify("Keys stored", "Restart dictate for the new keys to take effect."));
            }
        }
        MenuChoice::Quit => {
            with_app(|app| {
                app.session.log.event("quit", &[field("reason", "tray-menu"), field("utterances", app.session.utterance_count())])
            });
            unsafe { DestroyWindow(hwnd) };
        }
    }
}

fn open_config_folder() {
    let folder = paths::config_folder();
    let _ = std::fs::create_dir_all(&folder);

    let operation = wide("open");
    let target = wide(&folder.to_string_lossy());
    unsafe {
        ShellExecuteW(null_mut(), operation.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

impl App {
    /// FR-9.6 from the tray menu: switch now, and remember it in config.json
    /// so the next start uses the same microphone.
    fn select_microphone(&mut self, name: Option<String>) {
        let session = &mut self.session;
        session.config.input_device = name.clone();
        session.platform.recorder.set_input_device(name.clone());

        let shown = name.clone().unwrap_or_else(|| "the Windows default input".into());
        session.log.event("mic.selected", &[field("input", shown.as_str())]);

        match DictateConfig::set_string_in_file(&paths::config_file(), "InputDevice", name.as_deref()) {
            Ok(()) => session.platform.tray.notify("Microphone", &format!("dictate now records from {shown}.")),
            Err(error) => session.platform.tray.notify(
                "Microphone changed for this session only",
                &format!("Using {shown}, but config.json could not be updated: {error}"),
            ),
        }
    }
}

impl Platform for WindowsPlatform {
    type Target = HWND;

    fn focused(&self) -> HWND {
        window_inspector::foreground()
    }

    fn describe(&self, target: HWND, extra_console_processes: &[String]) -> TargetContext {
        window_inspector::describe(target, extra_console_processes)
    }

    fn has_input_device(&self) -> bool {
        Recorder::has_input_device()
    }

    fn start_capture(&mut self) {
        // Returns at once: the device opens off this thread, because this
        // thread also runs the keyboard hook. See audio.rs.
        self.recorder.start();
    }

    fn stop_capture(&mut self) -> Vec<u8> {
        self.recorder.stop()
    }

    fn take_capture_error(&mut self) -> Option<String> {
        self.recorder.take_open_error()
    }

    fn show_state(&mut self, state: SessionState) {
        self.tray.set_state(state);
        if self.show_overlay {
            self.overlay.show_state(state);
        }
    }

    fn notify(&self, title: &str, detail: &str) {
        self.tray.notify(title, detail);
    }

    fn type_text(&self, text: &str, config: &DictateConfig) -> Result<(), String> {
        injector::type_text(text, config.injection_chunk_size, config.injection_chunk_delay_ms)
    }

    fn copy_to_clipboard(&self, text: &str) -> Result<(), String> {
        clipboard::set_text(self.hwnd, text)
    }

    fn run_in_background(&self, job: Box<dyn FnOnce() + Send>) {
        let notify = SendHwnd::new(self.hwnd);
        thread::spawn(move || {
            job();
            notify.post(WM_PIPELINE_DONE, 0, 0);
        });
    }

    fn process_counters(&self) -> (u64, u32) {
        process_counters()
    }

    fn paste_hint(&self) -> &'static str {
        "Ctrl+V"
    }
}

fn process_counters() -> (u64, u32) {
    unsafe {
        let process = GetCurrentProcess();

        let mut memory: PROCESS_MEMORY_COUNTERS = zeroed();
        let size = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        memory.cb = size;
        let working_set = if K32GetProcessMemoryInfo(process, &mut memory, size) != 0 {
            memory.WorkingSetSize as u64 / (1024 * 1024)
        } else {
            0
        };

        let mut handles = 0u32;
        GetProcessHandleCount(process, &mut handles);

        (working_set, handles)
    }
}
