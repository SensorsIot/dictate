//! Collects the API keys the configured providers need and stores them in the
//! Keychain: the transcription key, and the cleanup key if that provider is a
//! different one.
//!
//! The dialogs are AppleScript's, shown by `osascript`: a native prompt with a
//! hidden-answer field and no window code of our own. The key comes back on
//! osascript's standard output, never on a command line.

use std::process::Command;

use dictate_core::DictateConfig;

use crate::credentials;
use crate::paths;

/// Asks for the keys. Returns true if the transcription key was stored. Blocks
/// until the user answers, so it never runs on the main thread once the menu
/// bar item exists.
pub fn prompt() -> bool {
    // A config file that does not parse is reported at startup; here it only
    // means asking for the default providers' keys.
    let config = DictateConfig::load(&paths::config_file()).unwrap_or_default();
    let transcription = config.transcription_provider.credential();
    let cleanup = config.cleanup_provider.credential();

    let Some(key) = ask(
        &format!("{transcription} API key (required).\n\nStored in your login Keychain, never in a file."),
        true,
    ) else {
        return false;
    };
    if key.is_empty() {
        alert(&format!("dictate needs a {transcription} key to transcribe. Nothing was stored."));
        return false;
    }

    // Asked only when cleanup uses a different provider's key.
    let cleanup_key = (cleanup != transcription).then(|| {
        ask(
            &format!(
                "{cleanup} API key (optional).\n\nWithout it, dictate types the transcript verbatim, without cleanup. \
                 Leave empty to skip."
            ),
            false,
        )
        .unwrap_or_default()
    });

    if let Err(error) = credentials::write(credentials::account(transcription), &key) {
        alert(&error);
        return false;
    }

    match cleanup_key {
        None => {}
        Some(value) if value.is_empty() => credentials::delete(credentials::account(cleanup)),
        Some(value) => {
            if let Err(error) = credentials::write(credentials::account(cleanup), &value) {
                alert(&error);
            }
        }
    }

    true
}

/// None when the user cancels.
fn ask(message: &str, required: bool) -> Option<String> {
    let cancel = if required { "Quit" } else { "Skip" };
    let script = format!(
        "text returned of (display dialog \"{}\" default answer \"\" with hidden answer \
         with title \"dictate\" buttons {{\"{cancel}\", \"OK\"}} default button \"OK\" cancel button \"{cancel}\")",
        escape(message)
    );

    let output = Command::new("osascript").arg("-e").arg(script).output().ok()?;
    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn alert(message: &str) {
    let script = format!(
        "display dialog \"{}\" with title \"dictate\" buttons {{\"OK\"}} default button \"OK\" with icon caution",
        escape(message)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
