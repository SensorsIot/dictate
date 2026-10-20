//! PCM in, deliverable text out. The whole platform-free half of dictate.
//!
//! Synchronous by design: a platform head runs one pipeline at a time on a
//! worker thread, so an async runtime would buy nothing but a dependency.

#![forbid(unsafe_code)]

pub mod audio_level;
pub mod cleaner;
pub mod cleanup_prompt;
pub mod config;
pub mod diagnostic_log;
mod error;
pub mod infomaniak;
pub mod language_guard;
pub mod model;
pub mod pipeline;
pub mod providers;
pub mod recent;
pub mod resample;
pub mod scribe;
pub mod session;
pub mod text_sanitizer;
pub mod wav;

pub use cleaner::{Cleaner, HaikuCleaner, PassthroughCleaner};
pub use config::DictateConfig;
pub use diagnostic_log::{field, DiagnosticLog, Field, LogValue};
pub use error::Error;
pub use infomaniak::{InfomaniakAccount, InfomaniakCleaner, InfomaniakTranscriber};
pub use language_guard::GermanOrEnglish;
pub use model::{
    CleanupProvider, Credential, LanguageMode, OverlayPosition, TargetContext, Transcript, TranscriptionProvider, Utterance,
    UtteranceStatus,
};
pub use pipeline::DictationPipeline;
pub use recent::RecentUtterances;
pub use scribe::{ScribeTranscriber, Transcriber};
pub use session::{Platform, Session, SessionState, StopReason};

/// Cuts `value` to `max` characters, marking the cut. Counts chars, not bytes,
/// so a German umlaut at the boundary cannot split a code point.
pub(crate) fn truncate(value: &str, max: usize) -> String {
    match value.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &value[..cut]),
        None => value.to_owned(),
    }
}
