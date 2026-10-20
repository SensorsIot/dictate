//! Builds the transcriber and cleaner the configuration asks for. The heads
//! only supply keys; which service gets which key is decided here, once.

use std::sync::Arc;
use std::time::Duration;

use crate::cleaner::{Cleaner, HaikuCleaner, PassthroughCleaner};
use crate::config::DictateConfig;
use crate::error::Error;
use crate::infomaniak::{InfomaniakAccount, InfomaniakCleaner, InfomaniakTranscriber};
use crate::model::{CleanupProvider, Credential, TranscriptionProvider};
use crate::scribe::{ScribeTranscriber, Transcriber};

/// `key` reads a stored credential. The transcription provider's key is
/// required; the cleanup provider's is not — without it dictation still works,
/// verbatim (FR-11.10).
pub fn build(
    config: &DictateConfig,
    key: impl Fn(Credential) -> Option<String>,
) -> Result<(Box<dyn Transcriber>, Box<dyn Cleaner>), Error> {
    // One account when both use Infomaniak, so the product lookup happens once.
    let mut infomaniak: Option<Arc<InfomaniakAccount>> = None;
    let mut account = |token: String| -> Result<Arc<InfomaniakAccount>, Error> {
        if let Some(existing) = &infomaniak {
            return Ok(Arc::clone(existing));
        }
        let created = InfomaniakAccount::new(token, config.infomaniak_product_id)?;
        infomaniak = Some(Arc::clone(&created));
        Ok(created)
    };

    let credential = config.transcription_provider.credential();
    let token = key(credential).ok_or_else(|| Error::new(format!("No {credential} key is stored.")))?;
    let transcriber: Box<dyn Transcriber> = match config.transcription_provider {
        TranscriptionProvider::ElevenLabs => {
            // Belt to the pipeline's braces: this bounds the socket.
            let timeout = Duration::from_secs(config.scribe_timeout_seconds.max(1) as u64 + 5);
            Box::new(ScribeTranscriber::new(token, config.effective_transcription_model(), timeout)?)
        }
        TranscriptionProvider::Infomaniak => Box::new(InfomaniakTranscriber::new(account(token)?, config)),
    };

    let cleaner: Box<dyn Cleaner> = match key(config.cleanup_provider.credential()) {
        None => Box::new(PassthroughCleaner),
        Some(token) => match config.cleanup_provider {
            CleanupProvider::Anthropic => Box::new(HaikuCleaner::new(token, config)?),
            CleanupProvider::Infomaniak => Box::new(InfomaniakCleaner::new(account(token)?, config)),
        },
    };

    Ok((transcriber, cleaner))
}
