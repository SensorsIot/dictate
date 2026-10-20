use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What had focus when the hotkey went down. Captured on the Windows side and
/// handed to the pipeline so cleanup can adapt formatting, and so the injector
/// knows whether it is typing into a console.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetContext {
    /// Executable name without extension, e.g. "wezterm-gui".
    pub process_name: String,
    /// Title bar text, often the best hint at what the user is doing.
    pub window_title: String,
    /// True for terminal-like windows, where a newline would press Enter.
    pub is_console: bool,
}

impl TargetContext {
    pub fn new(process_name: impl Into<String>, window_title: impl Into<String>, is_console: bool) -> Self {
        Self { process_name: process_name.into(), window_title: window_title.into(), is_console }
    }

    pub fn unknown() -> Self {
        Self::default()
    }
}

/// Serialises a unit-only enum by name and reads it back case-insensitively,
/// so a hand-edited `config.json` may say "german" as well as "German".
macro_rules! named_enum {
    ($type:ident { $($variant:ident),+ $(,)? }) => {
        impl $type {
            pub const ALL: &'static [$type] = &[$($type::$variant),+];

            pub fn name(self) -> &'static str {
                match self { $($type::$variant => stringify!($variant)),+ }
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.name())
            }
        }

        impl Serialize for $type {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::ALL
                    .iter()
                    .copied()
                    .find(|v| v.name().eq_ignore_ascii_case(&text))
                    .ok_or_else(|| serde::de::Error::custom(format!(
                        "unknown {} '{}'", stringify!($type), text)))
            }
        }
    };
}

/// Where the recording indicator sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayPosition {
    /// Lower right of the main screen, above the taskbar. Predictable — you learn where to glance.
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
    /// Follows the mouse. Close to where you are looking, but never in the same place twice.
    NearCursor,
}

named_enum!(OverlayPosition { BottomRight, BottomLeft, TopRight, TopLeft, NearCursor });

/// An API key dictate stores. The platform heads map each to a Credential
/// Manager target or Keychain account; the name doubles as its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    ElevenLabs,
    Anthropic,
    Infomaniak,
}

named_enum!(Credential { ElevenLabs, Anthropic, Infomaniak });

/// Who turns speech into text (FR-10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranscriptionProvider {
    /// ElevenLabs Scribe. The best on German and English mixed in one sentence.
    #[default]
    ElevenLabs,
    /// Infomaniak AI Services (Whisper), hosted in Switzerland. Slower: the
    /// API is asynchronous, so every utterance waits on a result poll.
    Infomaniak,
}

named_enum!(TranscriptionProvider { ElevenLabs, Infomaniak });

impl TranscriptionProvider {
    pub fn credential(self) -> Credential {
        match self {
            TranscriptionProvider::ElevenLabs => Credential::ElevenLabs,
            TranscriptionProvider::Infomaniak => Credential::Infomaniak,
        }
    }

    /// The model used when `TranscriptionModel` is not set.
    pub fn default_model(self) -> &'static str {
        match self {
            TranscriptionProvider::ElevenLabs => "scribe_v2",
            TranscriptionProvider::Infomaniak => "whisper",
        }
    }
}

/// Who cleans up the transcript (FR-11.1). Both get the same prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanupProvider {
    /// Claude through the Anthropic Messages API.
    #[default]
    Anthropic,
    /// Infomaniak AI Services, an OpenAI-compatible API hosted in Switzerland.
    Infomaniak,
}

named_enum!(CleanupProvider { Anthropic, Infomaniak });

impl CleanupProvider {
    pub fn credential(self) -> Credential {
        match self {
            CleanupProvider::Anthropic => Credential::Anthropic,
            CleanupProvider::Infomaniak => Credential::Infomaniak,
        }
    }

    /// The model used when `CleanupModel` is not set. Gemma, not Infomaniak's
    /// smaller Ministral: in testing Ministral translated the English half of
    /// a mixed sentence and Mistral Small answered a question (FR-11.4/11.5).
    pub fn default_model(self) -> &'static str {
        match self {
            CleanupProvider::Anthropic => "claude-haiku-4-5",
            CleanupProvider::Infomaniak => "google/gemma-4-31B-it",
        }
    }
}

/// Which language Scribe should assume for an utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LanguageMode {
    /// Let the provider decide. Wrong on short utterances often enough to want the override.
    #[default]
    Auto,
    German,
    English,
}

named_enum!(LanguageMode { Auto, German, English });

impl LanguageMode {
    /// ISO-639-1 code for the Scribe request, or `None` to let it auto-detect.
    pub fn to_language_code(self) -> Option<&'static str> {
        match self {
            LanguageMode::German => Some("de"),
            LanguageMode::English => Some("en"),
            LanguageMode::Auto => None,
        }
    }
}

/// The raw result of transcription, before cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// Verbatim transcript, fillers and all.
    pub text: String,
    /// What the provider detected, as it reports it, or the code we pinned.
    pub language_code: Option<String>,
}

impl Transcript {
    pub fn new(text: impl Into<String>, language_code: Option<&str>) -> Self {
        Self { text: text.into(), language_code: language_code.map(str::to_owned) }
    }
}

/// Why an utterance did not come back clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtteranceStatus {
    /// Transcribed and cleaned.
    Ok,
    /// Transcribed, but cleanup failed — `text` is the raw transcript.
    CleanupFailed,
    /// Nothing usable. `error` says why.
    Failed,
}

named_enum!(UtteranceStatus { Ok, CleanupFailed, Failed });

/// One press-to-release cycle, start to finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    pub status: UtteranceStatus,
    /// The text to deliver. Empty when `status` is `Failed`.
    pub text: String,
    /// The verbatim transcript, kept so the tray can offer it when cleanup went wrong.
    pub raw_text: String,
    pub language_code: Option<String>,
    pub error: Option<String>,
    pub transcribe_time: Duration,
    pub cleanup_time: Duration,
}

impl Utterance {
    pub fn fail(error: impl Into<String>) -> Self {
        Self {
            status: UtteranceStatus::Failed,
            text: String::new(),
            raw_text: String::new(),
            language_code: None,
            error: Some(error.into()),
            transcribe_time: Duration::ZERO,
            cleanup_time: Duration::ZERO,
        }
    }

    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}
