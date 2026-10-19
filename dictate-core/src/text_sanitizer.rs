//! Last gate before text becomes keystrokes. The rule that matters: a newline
//! synthesised into a console window is the Enter key, and Enter in a shell runs
//! whatever is on the line. Dictation must never be able to do that.

use std::path::Path;

use crate::model::TargetContext;

/// Terminal-ish executables, matched case-insensitively without extension.
const KNOWN_CONSOLE_PROCESSES: &[&str] = &[
    "wezterm-gui", "wezterm", "windowsterminal", "openconsole", "conhost",
    "cmd", "powershell", "pwsh", "putty", "kitty", "alacritty", "mintty",
    "terminal", "iterm2", "warp", "ghostty",
];

pub fn looks_like_console(process_name: &str, extra: &[String]) -> bool {
    if process_name.is_empty() {
        return false;
    }

    let name = Path::new(process_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(process_name)
        .to_lowercase();

    KNOWN_CONSOLE_PROCESSES.iter().any(|known| *known == name)
        || extra.iter().any(|candidate| candidate.to_lowercase() == name)
}

/// Swiss Standard German has no ß. Done here rather than in the cleanup
/// prompt so it holds for the raw-transcript fallback too (FR-7.1), and so
/// it does not depend on the model remembering.
pub fn to_swiss_spelling(text: &str) -> String {
    text.replace('ß', "ss").replace('ẞ', "SS")
}

/// Adds the separator between consecutive dictations, so "…run together."
/// and "Eins, zwei, drei" do not arrive as one word.
///
/// Applied to consoles too. What is dangerous in a shell is a newline, which
/// presses Enter and runs the line; a trailing space does nothing at all —
/// and a terminal is where dictation is used most, so excluding it removed
/// the separator from exactly the case that needed it.
pub fn with_trailing_space(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("{text} ")
    }
}

/// Makes `text` safe to type into `target`. In a console every line break
/// collapses to a single space; elsewhere line breaks are normalised to \r\n and
/// runs of blank lines are capped at one.
pub fn for_injection(text: &str, target: &TargetContext) -> String {
    if text.is_empty() {
        return String::new();
    }

    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");

    if target.is_console {
        let parts: Vec<&str> = normalised.split('\n').map(str::trim).filter(|p| !p.is_empty()).collect();
        return parts.join(" ").trim().to_owned();
    }

    let mut kept: Vec<&str> = Vec::new();
    let mut blank_run = 0;

    for line in normalised.split('\n') {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }

        kept.push(line.trim_end());
    }

    kept.join("\r\n").trim().to_owned()
}
