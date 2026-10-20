//! Collects the two API keys and stores them in the Keychain.
//!
//! The dialogs are AppleScript's, shown by `osascript`: a native prompt with a
//! hidden-answer field and no window code of our own. The key comes back on
//! osascript's standard output, never on a command line.

use std::process::Command;

use crate::credentials;

/// Asks for both keys. Returns true if the ElevenLabs key was stored. Blocks
/// until the user answers, so it never runs on the main thread once the menu
/// bar item exists.
pub fn prompt() -> bool {
    let Some(elevenlabs) = ask(
        "ElevenLabs API key (required).\n\nStored in your login Keychain, never in a file.",
        true,
    ) else {
        return false;
    };
    if elevenlabs.is_empty() {
        alert("dictate needs an ElevenLabs key to transcribe. Nothing was stored.");
        return false;
    }

    let anthropic = ask(
        "Anthropic API key (optional).\n\nWithout it, dictate types the transcript verbatim, without cleanup. \
         Leave empty to skip.",
        false,
    )
    .unwrap_or_default();

    if let Err(error) = credentials::write(credentials::ELEVENLABS, &elevenlabs) {
        alert(&error);
        return false;
    }

    if anthropic.is_empty() {
        credentials::delete(credentials::ANTHROPIC);
    } else if let Err(error) = credentials::write(credentials::ANTHROPIC, &anthropic) {
        alert(&error);
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
