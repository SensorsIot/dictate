//! Everything dictate tells the user. Each notice says what happened to their
//! words and what, if anything, they should do about it (FR-14.6). The
//! technical detail — status codes, device errors — belongs in the diagnostic
//! log, never here.

use crate::error::{Error, ErrorKind, Service};

/// A Windows balloon holds 255 characters of text and 63 of title; macOS
/// shows more. Written to fit the smaller.
pub const DETAIL_LIMIT: usize = 255;
pub const TITLE_LIMIT: usize = 63;

/// The service's own words are only quoted when this many characters of them
/// still fit — a stub of a sentence explains nothing.
const MINIMUM_QUOTE: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub detail: String,
}

impl Notice {
    pub fn new(title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { title: title.into(), detail: detail.into() }
    }
}

const NOT_TRANSCRIBED: &str = "Your words were not transcribed.";
const TYPED_AS_SPOKEN: &str = "Your words were typed as spoken, without cleanup.";

/// Transcription failed: the words are gone, so the advice ends in dictating again.
pub fn transcription_failed(error: &Error) -> Notice {
    let name = error.service().map_or("The transcription service", Service::name);
    let action = match error.kind() {
        ErrorKind::Overloaded | ErrorKind::ServerFault => {
            format!("This is a problem at {name}, not on your side. Wait a few minutes, then dictate again.")
        }
        ErrorKind::RateLimited => {
            format!("Wait a minute, then dictate again. If this keeps happening, your {name} plan allows too few requests.")
        }
        ErrorKind::OutOfCredit => format!("{} Then dictate again.", top_up(error)),
        ErrorKind::KeyRejected => re_enter_key(name),
        ErrorKind::Rejected => "A setting is probably wrong: check TranscriptionModel in config.json.".to_owned(),
        ErrorKind::Unreachable => "Check the internet connection, then dictate again.".to_owned(),
        ErrorKind::TimedOut => format!(
            "{name} did not answer in time. Wait a minute and dictate again; if every dictation times out, check the connection."
        ),
        ErrorKind::Other => "Please dictate again.".to_owned(),
    };
    with_quote(title(error, "Transcription failed"), format!("{NOT_TRANSCRIBED} {action}"), error)
}

/// Cleanup failed: the words were still typed (FR-7.1), so nothing needs
/// repeating — only the cause needs fixing, if it is the user's to fix.
pub fn cleanup_failed(error: &Error) -> Notice {
    let name = error.service().map_or("the cleanup service", Service::name);
    let action = match error.kind() {
        ErrorKind::Overloaded | ErrorKind::ServerFault => {
            format!("Nothing to do: cleanup resumes once {name} recovers.")
        }
        ErrorKind::RateLimited => {
            format!("Cleanup resumes in a moment. If this keeps happening, your {name} plan allows too few requests.")
        }
        ErrorKind::OutOfCredit => top_up(error),
        ErrorKind::KeyRejected => re_enter_key(name),
        ErrorKind::Rejected => "A setting is probably wrong: check CleanupModel in config.json.".to_owned(),
        ErrorKind::Unreachable => "Check the internet connection.".to_owned(),
        ErrorKind::TimedOut => {
            format!("{name} was slow to answer. If this keeps happening, raise CleanupTimeoutSeconds in config.json.")
        }
        ErrorKind::Other => String::new(),
    };
    let detail = if action.is_empty() { TYPED_AS_SPOKEN.to_owned() } else { format!("{TYPED_AS_SPOKEN} {action}") };
    with_quote(title(error, "Typed without cleanup"), detail, error)
}

pub fn cleanup_answered() -> Notice {
    Notice::new(
        "Typed without cleanup",
        "The cleanup model replied to your words instead of tidying them, so they were typed as spoken.",
    )
}

pub fn cleanup_empty() -> Notice {
    Notice::new("Typed without cleanup", "The cleanup model returned nothing, so your words were typed as spoken.")
}

pub fn silent_microphone() -> Notice {
    Notice::new(
        "The microphone is silent",
        "Nothing was transcribed: the microphone sent only silence. Check it is not muted, \
         or choose another in dictate's menu → Microphone.",
    )
}

pub fn nothing_recognised() -> Notice {
    Notice::new(
        "No speech recognised",
        "Nothing was typed. Start speaking after pressing the key, and keep holding it until you finish.",
    )
}

pub fn no_audio() -> Notice {
    Notice::new("Nothing was recorded", "The microphone delivered no audio. Hold the key a little longer and try again.")
}

pub fn internal_error() -> Notice {
    Notice::new(
        "Something went wrong in dictate",
        "This dictation was lost. Please dictate again. If it keeps happening, \
         turn on the diagnostic log and report it.",
    )
}

pub fn no_microphone() -> Notice {
    Notice::new("No microphone found", "Nothing was recorded. Connect a microphone, then press the key again.")
}

pub fn microphone_failed() -> Notice {
    Notice::new(
        "The microphone would not open",
        "Nothing was recorded. It may be unplugged or in use by another app. Plug it in, or choose another \
         in dictate's menu → Microphone, then press the key again.",
    )
}

pub fn limit_reached(seconds: i32) -> Notice {
    Notice::new(
        format!("Recording stopped at {seconds} s"),
        "That is the longest one dictation can be. What you said so far is being typed; \
         press the key again to go on.",
    )
}

pub fn focus_changed(paste_hint: &str) -> Notice {
    Notice::new(
        "Your text is on the clipboard",
        format!("You switched windows before it was ready, so nothing was typed. Press {paste_hint} where you want it."),
    )
}

/// `reason` comes from the platform, already written for the user; a trailing
/// `[…]` is the platform's detail for the log.
pub fn typing_failed(reason: &str, paste_hint: &str) -> Notice {
    let reason = reason.split(" [").next().unwrap_or(reason).trim();
    Notice::new("Your text is on the clipboard", format!("{reason} Press {paste_hint} to paste it."))
}

pub fn not_delivered() -> Notice {
    Notice::new(
        "Your text could not be delivered",
        "dictate could neither type it nor copy it. It is in dictate's menu → Recent.",
    )
}

fn title(error: &Error, fallback: &str) -> String {
    let Some(service) = error.service() else { return fallback.to_owned() };
    let name = service.name();
    match error.kind() {
        ErrorKind::Overloaded => format!("{name} is overloaded"),
        ErrorKind::ServerFault => format!("{name} is having problems"),
        ErrorKind::RateLimited => format!("Too many requests to {name}"),
        ErrorKind::OutOfCredit => format!("{name} credit used up"),
        ErrorKind::KeyRejected => format!("{name} rejected the API key"),
        ErrorKind::Rejected => format!("{name} rejected the request"),
        ErrorKind::Unreachable => format!("Cannot reach {name}"),
        ErrorKind::TimedOut => format!("{name} is not answering"),
        ErrorKind::Other => fallback.to_owned(),
    }
}

fn top_up(error: &Error) -> String {
    error.service().map_or("Top up your account with the provider.", Service::top_up).to_owned()
}

fn re_enter_key(name: &str) -> String {
    format!("Enter a new {name} key: dictate's menu → Re-enter API keys…, then restart dictate.")
}

/// Adds the service's own explanation when it fits (FR-7.4) — the quota
/// arithmetic, the list of valid models. Advice first: if anything is cut,
/// it is the quote.
fn with_quote(title: String, detail: String, error: &Error) -> Notice {
    let Some(said) = error.said() else { return Notice::new(title, detail) };
    let name = error.service().map_or("The service", Service::name);
    let frame = format!(" {name}: \"\"");
    let room = DETAIL_LIMIT.saturating_sub(detail.chars().count() + frame.chars().count());
    if room < MINIMUM_QUOTE {
        return Notice::new(title, detail);
    }
    let quote = if said.chars().count() <= room {
        said.to_owned()
    } else {
        let cut: String = said.chars().take(room - 1).collect();
        format!("{}…", cut.trim_end())
    };
    Notice::new(title, format!("{detail} {name}: \"{quote}\""))
}
