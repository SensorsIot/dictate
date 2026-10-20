//! Keeps automatic language detection to German and English (FR-10.8).
//!
//! Neither Scribe nor Whisper can be told "German or English only"; both
//! choose among about a hundred languages. So the first pass detects freely,
//! and only a result in some third language is redone — as German and as
//! English at once — keeping whichever the model is surer of. Forcing the
//! wrong one of the two does not translate: both providers still write what
//! was said, only with a lower log-probability, which is what is compared.

use std::thread;

use crate::error::Error;
use crate::model::{LanguageMode, Transcript};
use crate::scribe::Transcriber;

pub struct GermanOrEnglish {
    inner: Box<dyn Transcriber>,
}

impl GermanOrEnglish {
    pub fn new(inner: Box<dyn Transcriber>) -> Self {
        Self { inner }
    }

    /// The surer of the two forced runs. Without scores German wins, being the
    /// first of the pair; one failure leaves the other.
    pub fn pick(german: Result<Transcript, Error>, english: Result<Transcript, Error>) -> Result<Transcript, Error> {
        match (german, english) {
            (Ok(german), Ok(english)) => {
                let score = |t: &Transcript| t.confidence.unwrap_or(f64::NEG_INFINITY);
                Ok(if score(&english) > score(&german) { english } else { german })
            }
            (Ok(only), Err(_)) | (Err(_), Ok(only)) => Ok(only),
            (Err(error), Err(_)) => Err(error),
        }
    }
}

impl Transcriber for GermanOrEnglish {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        if language != LanguageMode::Auto {
            return self.inner.transcribe(wav, language).map(normalised);
        }

        let first = normalised(self.inner.transcribe(wav, LanguageMode::Auto)?);
        let accepted = match first.language_code.as_deref() {
            // A provider that did not say has nothing to correct.
            None => true,
            Some(code) => LanguageMode::from_reported(code).is_some(),
        };
        if accepted || first.text.is_empty() {
            return Ok(first);
        }

        let inner = &self.inner;
        let (german, english) = thread::scope(|scope| {
            let german = scope.spawn(|| inner.transcribe(wav, LanguageMode::German));
            let english = inner.transcribe(wav, LanguageMode::English);
            let german = german.join().unwrap_or_else(|_| Err(Error::new("German transcription panicked")));
            (german, english)
        });
        Self::pick(german.map(normalised), english.map(normalised))
    }
}

/// `de` / `en` whichever form the provider reported, so the log reads the same
/// for both. A third language is left as reported.
fn normalised(mut transcript: Transcript) -> Transcript {
    if let Some(mode) = transcript.language_code.as_deref().and_then(LanguageMode::from_reported) {
        transcript.language_code = mode.to_language_code().map(str::to_owned);
    }
    transcript
}
