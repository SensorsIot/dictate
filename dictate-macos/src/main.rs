//! dictate — push-to-talk dictation for macOS.

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod audio;
#[cfg(target_os = "macos")]
mod auth;
#[cfg(target_os = "macos")]
mod credentials;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod focus;
#[cfg(target_os = "macos")]
mod hotkey;
#[cfg(target_os = "macos")]
mod injector;
#[cfg(target_os = "macos")]
mod notify;
#[cfg(target_os = "macos")]
mod paths;
#[cfg(target_os = "macos")]
mod tray;

// The workspace is also built and tested on Linux and Windows, where only the
// core is meaningful. This keeps `cargo test --workspace` green there.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This build of dictate is for macOS.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    std::process::exit(start());
}

#[cfg(target_os = "macos")]
fn start() -> i32 {
    use std::fs::{self, OpenOptions};
    use std::time::Duration;

    use dictate_core::{cleaner, DiagnosticLog, DictateConfig, DictationPipeline, ScribeTranscriber};

    use crate::auth::alert;

    if std::env::args().skip(1).any(|a| a == "--auth" || a == "-a") {
        return if auth::prompt() { 0 } else { 1 };
    }

    // FR-5.4: one dictate per user session. Two would both react to the
    // hotkey. The lock is released by the system when the process exits, so
    // a crash never leaves it held.
    let _ = fs::create_dir_all(paths::config_folder());
    let instance = OpenOptions::new().create(true).truncate(false).write(true).open(paths::instance_lock());
    let _instance = match instance {
        Ok(file) if file.try_lock().is_ok() => file,
        Ok(_) => {
            alert("dictate is already running. Look for its dot in the menu bar.");
            return 0;
        }
        Err(error) => {
            alert(&format!("dictate could not start:\n\n{error}"));
            return 4;
        }
    };

    let config_file = paths::config_file();
    let config = match DictateConfig::load(&config_file) {
        Ok(config) => config,
        Err(error) => {
            // FR-16.2: a broken config file stops startup.
            alert(&format!(
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

    let scribe_timeout = Duration::from_secs(config.scribe_timeout_seconds.max(1) as u64 + 5);
    let transcriber = match ScribeTranscriber::new(elevenlabs_key, config.scribe_model_id.clone(), scribe_timeout) {
        Ok(transcriber) => transcriber,
        Err(error) => {
            alert(&format!("dictate could not start:\n\n{error}"));
            return 4;
        }
    };

    // FR-11.10: no cleanup key is a degraded mode, not a failure — you still
    // get dictation, just verbatim.
    let cleanup_key = credentials::read(credentials::cleanup_account(config.cleanup_provider));
    let cleaner = match cleaner::for_config(cleanup_key, &config) {
        Ok(cleaner) => cleaner,
        Err(error) => {
            alert(&format!("dictate could not start:\n\n{error}"));
            return 4;
        }
    };

    let pipeline = DictationPipeline::new(Box::new(transcriber), cleaner, config.clone());

    let log = if config.enable_diagnostic_log {
        DiagnosticLog::open_file(&paths::log_file(), 2 * 1024 * 1024).unwrap_or_else(|_| DiagnosticLog::disabled())
    } else {
        DiagnosticLog::disabled()
    };

    match app::run(config, pipeline, log) {
        Ok(()) => 0,
        Err(error) => {
            alert(&format!("dictate could not start:\n\n{error}"));
            4
        }
    }
}
