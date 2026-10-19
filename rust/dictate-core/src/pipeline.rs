use std::time::{Duration, Instant};

use crate::audio_level;
use crate::cleaner::Cleaner;
use crate::config::DictateConfig;
use crate::model::{TargetContext, Utterance, UtteranceStatus};
use crate::scribe::Transcriber;
use crate::text_sanitizer;
use crate::wav;

/// Cleanup removes fillers and adds punctuation; it never makes an utterance
/// much longer. Output beyond twice the input plus a little slack is not a
/// cleanup of it — in practice it is the model talking to the user, which on
/// the first desktop run happened for a one-second fragment (FR-11.12).
pub fn is_plausible_cleanup(raw: &str, cleaned: &str) -> bool {
    cleaned.chars().count() <= raw.chars().count() * 2 + 20
}

/// PCM in, deliverable text out.
///
/// Degradation policy, in order of preference:
///   Scribe fails  → nothing to say; report the error.
///   Cleanup fails → deliver the raw transcript. A scruffy sentence beats a lost one.
pub struct DictationPipeline {
    transcriber: Box<dyn Transcriber>,
    cleaner: Box<dyn Cleaner>,
    config: DictateConfig,
}

impl DictationPipeline {
    pub fn new(transcriber: Box<dyn Transcriber>, cleaner: Box<dyn Cleaner>, config: DictateConfig) -> Self {
        Self { transcriber, cleaner, config }
    }

    pub fn process(&self, pcm: &[u8], target: &TargetContext) -> Utterance {
        if pcm.is_empty() {
            return Utterance::fail("No audio was captured.");
        }

        // A device can return well-formed frames that are digital silence — a
        // USB interface still spinning up, a muted or wrong input. Catching it
        // here saves a billable upload and, more importantly, says microphone
        // rather than letting Scribe's empty result imply the speech was at
        // fault.
        if audio_level::is_silent(pcm, self.config.silence_threshold) {
            return Utterance::fail(format!(
                "The microphone produced only silence for {:.1}s. \
                 Check the input device, its gain, and that it is not muted.",
                wav::duration_of(pcm.len()).as_secs_f64()
            ));
        }

        let wav = wav::from_pcm16(pcm);

        let clock = Instant::now();
        let transcript = match self.transcriber.transcribe(&wav, self.config.language) {
            Ok(transcript) => transcript,
            Err(error) => return Utterance::fail(format!("Transcription failed: {error}")),
        };
        let transcribe_time = clock.elapsed();

        if transcript.text.is_empty() {
            return Utterance {
                status: UtteranceStatus::Failed,
                text: String::new(),
                raw_text: String::new(),
                language_code: transcript.language_code,
                error: Some("Nothing was recognised in the audio.".into()),
                transcribe_time,
                cleanup_time: Duration::ZERO,
            };
        }

        let clock = Instant::now();
        let (cleaned, cleanup_error) = match self.cleaner.clean(&transcript.text, target) {
            // An empty cleanup result means the model dropped the utterance —
            // treat that as a failure of cleanup, not as a silent deletion.
            Ok(cleaned) if cleaned.is_empty() => (transcript.text.clone(), Some("Cleanup returned nothing.".to_owned())),
            // FR-11.5, enforced rather than requested: a model that answers a
            // fragment ("I have no transcript to clean…") must not have its
            // answer typed into the user's window.
            Ok(cleaned) if !is_plausible_cleanup(&transcript.text, &cleaned) => (
                transcript.text.clone(),
                Some("Cleanup replied instead of cleaning; the transcript was delivered as spoken.".to_owned()),
            ),
            Ok(cleaned) => (cleaned, None),
            Err(error) => (transcript.text.clone(), Some(format!("Cleanup failed: {error}"))),
        };
        let cleanup_time = clock.elapsed();

        let cleaned = if self.config.swiss_spelling {
            text_sanitizer::to_swiss_spelling(&cleaned)
        } else {
            cleaned
        };

        Utterance {
            status: if cleanup_error.is_none() { UtteranceStatus::Ok } else { UtteranceStatus::CleanupFailed },
            text: text_sanitizer::for_injection(&cleaned, target),
            raw_text: transcript.text,
            language_code: transcript.language_code,
            error: cleanup_error,
            transcribe_time,
            cleanup_time,
        }
    }
}
