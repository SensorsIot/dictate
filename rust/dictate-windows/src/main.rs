//! dictate — push-to-talk dictation for Windows. The Rust port of
//! `src/Dictate.Windows`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod auth;
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
    use std::ptr::null_mut;
    use std::time::Duration;

    use dictate_core::{
        Cleaner, DiagnosticLog, DictateConfig, DictationPipeline, HaikuCleaner, PassthroughCleaner, ScribeTranscriber,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONWARNING};

    use crate::win::message_box;

    let fatal = |message: &str| message_box(null_mut(), message, MB_ICONERROR);

    if std::env::args().skip(1).any(|a| a == "--auth" || a == "-a") {
        return if auth::prompt() { 0 } else { 1 };
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

    let elevenlabs_key = match credentials::read(credentials::ELEVENLABS) {
        Some(key) => key,
        None => {
            if !auth::prompt() {
                return 3;
            }
            match credentials::read(credentials::ELEVENLABS) {
                Some(key) => key,
                None => return 3,
            }
        }
    };

    // Belt to the pipeline's braces: this bounds the socket, the pipeline
    // bounds the wait.
    let scribe_timeout = Duration::from_secs(config.scribe_timeout_seconds.max(1) as u64 + 5);
    let transcriber = match ScribeTranscriber::new(elevenlabs_key, config.scribe_model_id.clone(), scribe_timeout) {
        Ok(transcriber) => transcriber,
        Err(error) => {
            fatal(&format!("dictate could not start:\n\n{error}"));
            return 4;
        }
    };

    // FR-11.10: no Anthropic key is a degraded mode, not a failure — you still
    // get dictation, just verbatim.
    let cleaner: Box<dyn Cleaner> = match credentials::read(credentials::ANTHROPIC) {
        Some(key) => match HaikuCleaner::new(key, &config) {
            Ok(cleaner) => Box::new(cleaner),
            Err(error) => {
                fatal(&format!("dictate could not start:\n\n{error}"));
                return 4;
            }
        },
        None => Box::new(PassthroughCleaner),
    };

    let pipeline = DictationPipeline::new(Box::new(transcriber), cleaner, config.clone());

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
