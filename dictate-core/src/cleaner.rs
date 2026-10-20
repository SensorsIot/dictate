use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use crate::cleanup_prompt;
use crate::config::DictateConfig;
use crate::error::Error;
use crate::model::TargetContext;
use crate::truncate;

pub trait Cleaner: Send + Sync {
    fn clean(&self, transcript: &str, target: &TargetContext) -> Result<String, Error>;
}

pub const MESSAGES_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2023-06-01";

/// Cleanup via Claude Haiku — the tier that is cheap and fast enough to sit in a
/// push-to-talk loop.
///
/// Plain HTTP against the Messages API rather than an SDK: there is no official
/// Rust one, and the request is small enough that owning it costs less than
/// tracking someone else's client surface.
pub struct HaikuCleaner {
    http: Client,
    api_key: String,
    model: String,
    // Built once: it is identical for every utterance.
    //
    // No cache_control on purpose. Haiku 4.5's minimum cacheable prefix is 4096
    // tokens and this prompt is a few hundred, so a breakpoint would be silently
    // ignored. Revisit if the vocabulary ever grows past that threshold.
    system_prompt: String,
}

impl HaikuCleaner {
    pub fn new(api_key: impl Into<String>, config: &DictateConfig) -> Result<Self, Error> {
        let timeout = Duration::from_secs(config.cleanup_timeout_seconds.max(1) as u64);
        Ok(Self {
            http: Client::builder().timeout(timeout).build()?,
            api_key: api_key.into(),
            model: config.cleanup_model.clone(),
            system_prompt: cleanup_prompt::build_system(config),
        })
    }

    pub fn request_body(model: &str, system_prompt: &str, transcript: &str, target: &TargetContext) -> Value {
        json!({
            "model": model,
            // Cleanup only ever shortens the transcript. Generous headroom for
            // German compounds, still far below anything that could run long.
            "max_tokens": 2048,
            // Deterministic: the same utterance should not clean up two ways (FR-11.8).
            "temperature": 0,
            "system": system_prompt,
            "messages": [
                { "role": "user", "content": cleanup_prompt::build_user(transcript, target) }
            ]
        })
    }

    /// Concatenates the text blocks of a Messages API response.
    pub fn extract_text(json: &str) -> Result<String, Error> {
        let root: Value = serde_json::from_str(json)
            .map_err(|e| Error::new(format!("Cleanup response was not JSON ({e}): {}", truncate(json, 300))))?;

        let blocks = root
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new(format!("Cleanup response had no content: {}", truncate(json, 300))))?;

        let text: String = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect();

        Ok(text.trim().to_owned())
    }
}

impl Cleaner for HaikuCleaner {
    fn clean(&self, transcript: &str, target: &TargetContext) -> Result<String, Error> {
        let body = Self::request_body(&self.model, &self.system_prompt, transcript, target);

        let response = self
            .http
            .post(MESSAGES_ENDPOINT)
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", API_VERSION)
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()?;

        let status = response.status();
        let text = response.text()?;

        if !status.is_success() {
            return Err(Error::new(format!(
                "Anthropic returned {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                truncate(&text, 500)
            )));
        }

        Self::extract_text(&text)
    }
}

/// Cleanup that does nothing. Used when no Anthropic key is configured, so the
/// tool still dictates — just verbatim (FR-11.10).
pub struct PassthroughCleaner;

impl Cleaner for PassthroughCleaner {
    fn clean(&self, transcript: &str, _target: &TargetContext) -> Result<String, Error> {
        Ok(transcript.to_owned())
    }
}
