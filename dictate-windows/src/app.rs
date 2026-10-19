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
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use dictate_core::text_sanitizer;
use dictate_core::{field, DiagnosticLog, DictateConfig, DictationPipeline, RecentUtterances, TargetContext, Utterance, UtteranceStatus};
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
use crate::overlay::{Overlay, SessionState};
use crate::tray::{self, MenuChoice, Tray};
use crate::win::{
    instance, message_box, wide, SendHwnd, WM_HOTKEY_DOWN, WM_HOTKEY_REJECTED, WM_HOTKEY_UP, WM_PIPELINE_DONE,
    WM_RECORDER_CLOSED, WM_RECORDER_FIRST_BUFFER, WM_RECORDER_LIMIT, WM_RECORDER_OPEN_FAILED, WM_TRAY,
};
use crate::{auth, clipboard, hotkey, injector, paths, window_inspector};

/// How many delivered utterances stay recoverable. In memory only (FR-17.2).
const RECENT_CAPACITY: usize = 5;

const TIMER_RECONCILE: usize = 1;

/// Why capture ended. Recorded on every `recording.stop`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StopReason {
    /// The hotkey came up — the normal path.
    Released,
    /// No key-up arrived; the key was observed to be physically up.
    Reconciled,
    /// MaximumRecordingSeconds was hit.
    Limit,
}

impl StopReason {
    fn name(self) -> &'static str {
        match self {
            StopReason::Released => "Released",
            StopReason::Reconciled => "Reconciled",
            StopReason::Limit => "Limit",
        }
    }
}

/// What the main thread remembers about an utterance while the pipeline runs.
struct Pending {
    held: Duration,
    pcm_bytes: usize,
    target: TargetContext,
}

struct App {
    hwnd: HWND,
    config: DictateConfig,
    pipeline: Arc<DictationPipeline>,
    recorder: Recorder,
    overlay: Overlay,
    tray: Tray,
    recent: RecentUtterances,
    log: Arc<DiagnosticLog>,
    utterance_count: u64,
    state: SessionState,
    pinned_window: HWND,
    pressed_at: Instant,
    pending: Option<Pending>,
    results_tx: Sender<Utterance>,
    results: Receiver<Utterance>,
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

    let (results_tx, results) = mpsc::channel();

    let started_fields = [
        field("version", env!("CARGO_PKG_VERSION")),
        field("hotkey", format!("0x{:02X}", config.hotkey_virtual_key)),
        field("keepMicOpen", config.keep_microphone_open),
        field("input", resolve_input(config.input_device.as_deref()).1),
        field("ignoreInjected", config.ignore_injected_hotkey),
        field("cleanup", config.cleanup_model.as_str()),
    ];

    let hotkey_key = config.hotkey_virtual_key;
    let suppress = config.suppress_hotkey;
    let ignore_injected = config.ignore_injected_hotkey;

    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            hwnd,
            config,
            pipeline: Arc::new(pipeline),
            recorder,
            overlay,
            tray,
            recent: RecentUtterances::new(RECENT_CAPACITY),
            log: Arc::clone(&log),
            utterance_count: 0,
            state: SessionState::Idle,
            pinned_window: null_mut(),
            pressed_at: Instant::now(),
            pending: None,
            results_tx,
            results,
        });
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
    hotkey::uninstall();
    unsafe {
        KillTimer(hwnd, TIMER_RECONCILE);
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
        with_app(|app| app.tray.add());
        return Some(0);
    }

    match message {
        WM_HOTKEY_DOWN => {
            with_app(|app| app.begin_session(wparam));
        }
        WM_HOTKEY_UP => {
            let reason = if wparam == hotkey::RELEASE_RECONCILED { StopReason::Reconciled } else { StopReason::Released };
            with_app(|app| app.end_session(reason));
        }
        WM_HOTKEY_REJECTED => {
            with_app(|app| {
                app.log.event(
                    "hotkey.rejected",
                    &[field("reason", "injected"), field("lowerIL", wparam & hotkey::ORIGIN_LOWER_INTEGRITY != 0)],
                )
            });
        }
        WM_RECORDER_LIMIT => {
            with_app(|app| app.end_session(StopReason::Limit));
        }
        WM_RECORDER_FIRST_BUFFER => {
            with_app(|app| app.log.event("mic.firstBuffer", &[field("after", Duration::from_millis(wparam as u64))]));
        }
        WM_RECORDER_CLOSED => {
            with_app(|app| app.log.event("mic.close", &[field("confirmed", wparam != 0)]));
        }
        WM_RECORDER_OPEN_FAILED => {
            with_app(App::on_open_failed);
        }
        WM_PIPELINE_DONE => {
            with_app(App::finish_session);
        }
        WM_TIMER if wparam == TIMER_RECONCILE => {
            hotkey::reconcile();
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
        (app.recent.items().into_iter().map(|u| u.text).collect(), app.config.input_device.clone())
    })
    .unwrap_or_default();

    let inputs = audio::list_inputs();
    let selected = configured.as_deref().and_then(|wanted| audio::find_input(&inputs, wanted));

    match tray::show_menu(hwnd, &recent, &inputs, selected) {
        MenuChoice::Nothing => {}
        MenuChoice::Microphone(choice) => {
            let name = choice.and_then(|index| inputs.get(index).cloned());
            with_app(|app| app.select_microphone(name));
        }
        MenuChoice::Recent(index) => {
            if let Some(text) = recent.get(index) {
                if let Err(error) = clipboard::set_text(hwnd, text) {
                    with_app(|app| app.tray.notify("Could not copy", &error));
                }
            }
        }
        MenuChoice::OpenConfigFolder => open_config_folder(),
        MenuChoice::ReEnterKeys => {
            if auth::prompt() {
                with_app(|app| app.tray.notify("Keys stored", "Restart dictate for the new keys to take effect."));
            }
        }
        MenuChoice::Quit => {
            with_app(|app| {
                app.log.event("quit", &[field("reason", "tray-menu"), field("utterances", app.utterance_count)])
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
    fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.tray.set_state(state);

        if self.config.show_overlay {
            self.overlay.show_state(state);
        }
    }

    fn begin_session(&mut self, origin: usize) {
        // FR-5.1: one session at a time. A press while transcribing is ignored
        // rather than queued — two utterances in flight is how audio from one
        // ends up delivered into the target of the other.
        if self.state != SessionState::Idle {
            return;
        }

        if !Recorder::has_input_device() {
            self.tray.notify("No microphone", "dictate found no audio input device.");
            return;
        }

        // FR-6.1: the target window is recorded now and never re-read. Only the
        // handle — resolving it to a process name can wait until the pipeline
        // is already running.
        self.pinned_window = window_inspector::foreground();
        self.pressed_at = hotkey::pressed_at().unwrap_or_else(Instant::now);

        // Returns at once: the device opens off this thread, because this
        // thread also runs the keyboard hook. See audio.rs.
        self.recorder.start();

        let latency = self.pressed_at.elapsed();
        self.set_state(SessionState::Recording);

        // The origin is recorded because a press dictate did not expect is
        // otherwise indistinguishable from the user's thumb.
        self.log.event(
            "recording.start",
            &[
                field("pressToStart", latency),
                field("injected", origin & hotkey::ORIGIN_INJECTED != 0),
                field("lowerIL", origin & hotkey::ORIGIN_LOWER_INTEGRITY != 0),
            ],
        );
    }

    /// FR-9.6 from the tray menu: switch now, and remember it in config.json
    /// so the next start uses the same microphone.
    fn select_microphone(&mut self, name: Option<String>) {
        self.config.input_device = name.clone();
        self.recorder.set_input_device(name.clone());

        let shown = name.clone().unwrap_or_else(|| "the Windows default input".into());
        self.log.event("mic.selected", &[field("input", shown.as_str())]);

        match DictateConfig::set_string_in_file(&paths::config_file(), "InputDevice", name.as_deref()) {
            Ok(()) => self.tray.notify("Microphone", &format!("dictate now records from {shown}.")),
            Err(error) => self.tray.notify(
                "Microphone changed for this session only",
                &format!("Using {shown}, but config.json could not be updated: {error}"),
            ),
        }
    }

    /// The recorder could not open a device for the session in progress and
    /// has already ended its side of it.
    fn on_open_failed(&mut self) {
        let error = self.recorder.take_open_error().unwrap_or_else(|| "Unknown error.".into());
        self.log.event("mic.openFailed", &[field("error", error.as_str())]);

        if self.state == SessionState::Recording {
            self.set_state(SessionState::Idle);
        }

        self.tray.notify("Could not start recording", &error);
    }

    fn end_session(&mut self, reason: StopReason) {
        if self.state != SessionState::Recording {
            return;
        }

        let held = self.pressed_at.elapsed();
        let pcm = self.recorder.stop();

        self.log.event(
            "recording.stop",
            &[field("reason", reason.name()), field("held", held), field("audioMB", megabytes(pcm.len()))],
        );

        // FR-5.2: an accidental tap costs nothing and says nothing — to the
        // user. It is still logged, or a session that ends here is invisible.
        if held.as_millis() < self.config.minimum_hold_ms.max(0) as u128 {
            self.set_state(SessionState::Idle);
            self.log.event("recording.discarded", &[field("reason", "tap"), field("held", held)]);
            return;
        }

        if reason == StopReason::Limit {
            self.tray.notify(
                "Recording limit reached",
                &format!(
                    "Stopped after {}s. Transcribing what was captured.",
                    self.config.maximum_recording_seconds
                ),
            );
        }

        self.set_state(SessionState::Transcribing);

        // Deferred from press time: a process lookup, which here runs while the
        // user is already waiting for the network anyway.
        let target = window_inspector::describe(self.pinned_window, &self.config.extra_console_processes);

        self.pending = Some(Pending { held, pcm_bytes: pcm.len(), target: target.clone() });

        let pipeline = Arc::clone(&self.pipeline);
        let results = self.results_tx.clone();
        let notify = SendHwnd::new(self.hwnd);

        thread::spawn(move || {
            // The pipeline degrades internally; a panic means something outside
            // its contract broke. It costs one utterance, never the session.
            let utterance = panic::catch_unwind(AssertUnwindSafe(|| pipeline.process(&pcm, &target)))
                .unwrap_or_else(|_| Utterance::fail("The pipeline hit an internal error."));
            let _ = results.send(utterance);
            notify.post(WM_PIPELINE_DONE, 0, 0);
        });
    }

    fn finish_session(&mut self) {
        let Ok(utterance) = self.results.try_recv() else { return };
        let Some(pending) = self.pending.take() else { return };

        self.set_state(SessionState::Idle);
        self.utterance_count += 1;
        self.log_outcome(&utterance, &pending);
        self.deliver(&utterance);
    }

    /// Records the shape of an utterance and the process counters alongside it.
    /// Length rather than text: enough to correlate with a delivery problem,
    /// and FR-17.1 stays intact.
    fn log_outcome(&self, utterance: &Utterance, pending: &Pending) {
        if !self.log.is_enabled() {
            return;
        }

        let (working_set_mb, handles) = process_counters();

        self.log.event(
            "utterance",
            &[
                field("n", self.utterance_count),
                field("held", pending.held),
                field("audioMB", megabytes(pending.pcm_bytes)),
                field("status", utterance.status),
                field("chars", utterance.text.chars().count()),
                field("lang", utterance.language_code.as_deref()),
                field("target", pending.target.process_name.as_str()),
                field("console", pending.target.is_console),
                field("transcribe", utterance.transcribe_time),
                field("cleanup", utterance.cleanup_time),
                field("error", utterance.error.as_deref()),
                field("wsMB", working_set_mb),
                field("handles", handles),
            ],
        );
    }

    fn deliver(&mut self, utterance: &Utterance) {
        if utterance.status == UtteranceStatus::Failed {
            self.tray.notify("Dictation failed", utterance.error.as_deref().unwrap_or("Unknown error."));
            return;
        }

        if !utterance.has_text() {
            return;
        }

        self.recent.add(utterance.clone());

        if utterance.status == UtteranceStatus::CleanupFailed {
            // Delivered verbatim rather than lost (FR-7.1), but the user is told
            // so an unusually rough sentence is explained.
            self.tray.notify("Delivered without cleanup", utterance.error.as_deref().unwrap_or("Cleanup failed."));
        }

        // Before delivery, not after: if typing fails, or lands somewhere the
        // user did not intend, the text is already one Ctrl+V away. This covers
        // the case the focus check cannot see — the wrong window was focused
        // the whole time.
        if self.config.always_copy_to_clipboard {
            if let Err(error) = clipboard::set_text(self.hwnd, &utterance.text) {
                self.log.event("clipboard.failed", &[field("error", error)]);
            }
        }

        // FR-6.2: the window that had focus at press time, or nothing.
        if window_inspector::foreground() != self.pinned_window {
            self.log.event("delivery", &[field("method", "clipboard"), field("reason", "focus-changed")]);
            self.to_clipboard(&utterance.text, "Focus changed", "The text is on the clipboard — press Ctrl+V.");
            return;
        }

        // A trailing space so consecutive dictations do not run together. Not
        // added on the clipboard path, where the user sees what they paste.
        let typed = if self.config.append_space_after_insert {
            text_sanitizer::with_trailing_space(&utterance.text)
        } else {
            utterance.text.clone()
        };

        match injector::type_text(&typed, self.config.injection_chunk_size, self.config.injection_chunk_delay_ms) {
            Ok(()) => {
                self.log.event("delivery", &[field("method", "typed"), field("chars", typed.chars().count())]);
            }
            Err(error) => {
                self.log.event("delivery.failed", &[field("error", error.as_str())]);
                self.to_clipboard(&utterance.text, "Could not type the text", &error);
            }
        }
    }

    fn to_clipboard(&self, text: &str, title: &str, detail: &str) {
        match clipboard::set_text(self.hwnd, text) {
            Ok(()) => self.tray.notify(title, detail),
            Err(error) => {
                self.tray.notify("Text could not be delivered", &format!("{detail} Clipboard also failed: {error}"))
            }
        }
    }
}

fn megabytes(bytes: usize) -> f64 {
    (bytes as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0
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
