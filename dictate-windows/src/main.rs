//! dictate — push-to-talk dictation for Windows.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod auth;
#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod credentials;
#[cfg(windows)]
mod hotkey;
#[cfg(windows)]
mod injector;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod paths;
#[cfg(windows)]
mod start_guard;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod window_inspector;

// The workspace is also built and tested on Linux, where only the core is
// meaningful. This keeps `cargo test --workspace` green there.
#[cfg(not(windows))]
fn main() {
    eprintln!("dictate is a Windows application.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    std::process::exit(start());
}

#[cfg(windows)]
fn start() -> i32 {
    use std::ptr::{null, null_mut};

    use dictate_core::{providers, DiagnosticLog, DictateConfig, DictationPipeline};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING};

    use crate::win::{message_box, wide};

    let fatal = |message: &str| message_box(null_mut(), message, MB_ICONERROR);

    if std::env::args().skip(1).any(|a| a == "--auth" || a == "-a") {
        return if auth::prompt() { 0 } else { 1 };
    }

    // FR-5.4: one dictate per user session. Two would both react to the
    // hotkey, and once dictate can register itself at login a leftover
    // Startup-folder shortcut is enough to start a second. The handle is held
    // until the process exits; Windows releases it then.
    let instance_name = wide("Local\\dictate.single-instance");
    let _instance = unsafe { CreateMutexW(null(), 0, instance_name.as_ptr()) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        message_box(
            null_mut(),
            "dictate is already running. Look for its icon in the notification area.",
            MB_ICONINFORMATION,
        );
        return 0;
    }

    let config_file = paths::config_file();
    let config = match DictateConfig::load(&config_file) {
        Ok(config) => config,
        Err(error) => {
            // FR-16.2: a broken config file stops startup. Falling back to
            // defaults would look exactly like the settings never applying.
            fatal(&format!(
                "{} could not be read:\n\n{error}\n\nFix or delete the file and start dictate again.",
                config_file.display()
            ));
            return 2;
        }
    };

    // The transcription provider's key is the one dictate cannot run without.
    let transcription_key = credentials::target(config.transcription_provider.credential());
    if credentials::read(transcription_key).is_none() && (!auth::prompt() || credentials::read(transcription_key).is_none()) {
        return 3;
    }

    let (transcriber, cleaner) = match providers::build(&config, |c| credentials::read(credentials::target(c))) {
        Ok(services) => services,
        Err(error) => {
            fatal(&format!("dictate could not start:\n\n{error}"));
            return 4;
        }
    };

    let pipeline = DictationPipeline::new(transcriber, cleaner, config.clone());

    let log = if config.enable_diagnostic_log {
        match DiagnosticLog::open_file(&paths::log_file(), 2 * 1024 * 1024) {
            Ok(log) => log,
            Err(error) => {
                // A log that cannot be opened is not a reason to refuse to run.
                message_box(
                    null_mut(),
                    &format!("Diagnostic log could not be opened, continuing without it:\n\n{error}"),
                    MB_ICONWARNING,
                );
                DiagnosticLog::disabled()
            }
        }
    } else {
        DiagnosticLog::disabled()
    };

    match app::run(config, pipeline, log) {
        Ok(()) => 0,
        Err(error) => {
            fatal(&format!("dictate could not start:\n\n{error}"));
            4
        }
    }
}
