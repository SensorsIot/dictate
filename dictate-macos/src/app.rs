//! The main thread: the AppKit run loop, the menu-bar item, and the session.
//!
//! The session lives in a thread-local on the main thread. The event tap, the
//! audio worker and the pipeline thread never touch it; they `post` closures to
//! the main queue and the work happens there, in order.

#![allow(unused_unsafe)]

use std::cell::RefCell;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use dictate_core::{
    field, DiagnosticLog, DictateConfig, DictationPipeline, Platform, Session, SessionState, StopReason,
    TargetContext,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use tray_icon::menu::{MenuEvent, MenuId};

use crate::audio::{self, Recorder, RecorderEvent};
use crate::ffi::*;
use crate::focus::{self, Window};
use crate::tray::{MenuChoice, Tray};
use crate::{auth, hotkey, injector, notify, paths};

/// The macOS half of a session: what `Session` asks the operating system for.
pub struct MacPlatform {
    recorder: Recorder,
    tray: Tray,
}

thread_local! {
    static APP: RefCell<Option<Session<MacPlatform>>> = const { RefCell::new(None) };
}

/// Runs `job` on the main thread, after whatever is already queued there.
pub fn post(job: impl FnOnce() + Send + 'static) {
    dispatch::Queue::main().exec_async(job);
}

fn with_session<R>(job: impl FnOnce(&mut Session<MacPlatform>) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().map(job)
    })
}

pub fn run(config: DictateConfig, pipeline: DictationPipeline, log: DiagnosticLog) -> Result<(), String> {
    let main_thread = MainThreadMarker::new().ok_or("dictate must start on the main thread.")?;
    let application = NSApplication::sharedApplication(main_thread);
    // A menu-bar utility: no Dock icon, never the active application.
    unsafe { application.setActivationPolicy(NSApplicationActivationPolicy::Accessory) };

    let key = hotkey::mac_key(config.hotkey_virtual_key).ok_or_else(|| {
        format!(
            "HotkeyVirtualKey 0x{:02X} has no Mac equivalent. Use a modifier (0x5C Right Command, 0xA5 Right \
             Option, …) or F1–F20.",
            config.hotkey_virtual_key
        )
    })?;

    let log = Arc::new(log);
    log.event(
        "started",
        &[
            field("version", env!("CARGO_PKG_VERSION")),
            field("hotkey", format!("0x{:02X}", config.hotkey_virtual_key)),
            field("keycode", key.keycode),
            field("keepMicOpen", config.keep_microphone_open),
            field("input", config.input_device.as_deref().unwrap_or("system default")),
            field("ignoreInjected", config.ignore_injected_hotkey),
            field("cleanup", config.cleanup_model.as_str()),
        ],
    );

    let recorder_log = Arc::clone(&log);
    let recorder = Recorder::new(
        config.maximum_recording_seconds,
        config.keep_microphone_open,
        config.input_device.clone(),
        Arc::new(move |event| match event {
            RecorderEvent::LimitReached => post(|| {
                with_session(|session| session.end(StopReason::Limit));
            }),
            RecorderEvent::OpenFailed => post(|| {
                with_session(|session| session.capture_failed());
            }),
            RecorderEvent::FirstBuffer(after) => recorder_log.event("mic.firstBuffer", &[field("after", after)]),
            RecorderEvent::DeviceOpened(name) => recorder_log.event("mic.open", &[field("input", name)]),
            RecorderEvent::DeviceClosed => recorder_log.event("mic.close", &[]),
            RecorderEvent::StreamError(error) => recorder_log.event("mic.error", &[field("error", error)]),
        }),
    );
    recorder.pre_warm();

    let tray = Tray::new()?;
    let session = Session::new(MacPlatform { recorder, tray }, config, pipeline, log);
    APP.with(|cell| *cell.borrow_mut() = Some(session));
    with_session(refresh_menu);

    MenuEvent::set_event_handler(Some(|event: MenuEvent| post(move || on_menu(event.id))));

    check_permissions();
    hotkey::start(key);

    unsafe { application.run() };
    Ok(())
}

pub fn on_press(pressed_at: Instant, injected: bool) {
    with_session(|session| {
        // FR-8.6: the origin is always recorded; rejecting it is a setting.
        if injected && session.config.ignore_injected_hotkey {
            session.log.event("hotkey.rejected", &[field("injected", true)]);
            return;
        }
        session.begin(pressed_at, vec![field("injected", injected)]);
    });
}

pub fn on_release(reason: StopReason) {
    with_session(|session| session.end(reason));
}

pub fn log_event(name: &'static str, ok: bool) {
    with_session(|session| session.log.event(name, &[field("ok", ok)]));
}

pub fn notify(title: &str, detail: &str) {
    notify::show(title, detail);
}

/// Accessibility lets dictate type and read window titles. Asking for it here
/// makes macOS show its own prompt and list dictate in System Settings, so the
/// user has a switch to turn on rather than an app to find.
fn check_permissions() {
    let trusted = unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let trusted = AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            CFRelease(options);
        }
        trusted
    };

    let listening = unsafe { CGPreflightListenEventAccess() };
    with_session(|session| {
        session.log.event("permissions", &[field("accessibility", trusted), field("inputMonitoring", listening)])
    });
}

fn refresh_menu(session: &mut Session<MacPlatform>) {
    let recent: Vec<String> = session.recent.items().into_iter().map(|utterance| utterance.text).collect();
    let inputs = audio::list_inputs();
    let selected = session.config.input_device.as_deref().and_then(|wanted| audio::find_input(&inputs, wanted));
    session.platform.tray.rebuild(&recent, &inputs, selected);
}

fn on_menu(id: MenuId) {
    let Some(choice) = with_session(|session| session.platform.tray.choice(&id)).flatten() else { return };

    match choice {
        MenuChoice::Recent(text) => {
            if let Err(error) = copy_to_clipboard(&text) {
                notify("Could not copy", &error);
            }
        }
        MenuChoice::Microphone(name) => {
            with_session(|session| select_microphone(session, name));
        }
        MenuChoice::OpenConfigFolder => {
            let folder = paths::config_folder();
            let _ = std::fs::create_dir_all(&folder);
            let _ = Command::new("open").arg(folder).spawn();
        }
        MenuChoice::ReEnterKeys => {
            // The dialogs block until answered; the main thread must not.
            thread::spawn(|| {
                if auth::prompt() {
                    notify("Keys stored", "Quit and start dictate again for the new keys to take effect.");
                }
            });
        }
        MenuChoice::Quit => {
            with_session(|session| {
                session.log.event("quit", &[field("reason", "menu"), field("utterances", session.utterance_count())])
            });
            std::process::exit(0);
        }
    }
}

/// FR-9.6 from the menu: switch now, and remember it in config.json so the
/// next start uses the same microphone.
fn select_microphone(session: &mut Session<MacPlatform>, name: Option<String>) {
    session.config.input_device = name.clone();
    session.platform.recorder.set_input_device(name.clone());

    let shown = name.clone().unwrap_or_else(|| "the system default input".into());
    session.log.event("mic.selected", &[field("input", shown.as_str())]);

    match DictateConfig::set_string_in_file(&paths::config_file(), "InputDevice", name.as_deref()) {
        Ok(()) => notify("Microphone", &format!("dictate now records from {shown}.")),
        Err(error) => notify(
            "Microphone changed for this session only",
            &format!("Using {shown}, but config.json could not be updated: {error}"),
        ),
    }

    refresh_menu(session);
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .map_err(|error| error.to_string())
}

impl Platform for MacPlatform {
    type Target = Window;

    fn focused(&self) -> Window {
        focus::foreground()
    }

    fn describe(&self, target: Window, extra_console_processes: &[String]) -> TargetContext {
        focus::describe(target, extra_console_processes)
    }

    fn has_input_device(&self) -> bool {
        Recorder::has_input_device()
    }

    fn start_capture(&mut self) {
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
    }

    fn notify(&self, title: &str, detail: &str) {
        notify::show(title, detail);
    }

    fn type_text(&self, text: &str, config: &DictateConfig) -> Result<(), String> {
        injector::type_text(text, config.injection_chunk_size, config.injection_chunk_delay_ms)
    }

    fn copy_to_clipboard(&self, text: &str) -> Result<(), String> {
        copy_to_clipboard(text)
    }

    fn run_in_background(&self, job: Box<dyn FnOnce() + Send>) {
        thread::spawn(move || {
            job();
            post(|| {
                with_session(|session| {
                    session.finish();
                    refresh_menu(session);
                });
            });
        });
    }

    fn paste_hint(&self) -> &'static str {
        "⌘V"
    }
}
