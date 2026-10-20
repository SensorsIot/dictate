use std::time::Duration;

use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::error::Error;
use crate::model::{LanguageMode, Transcript};
use crate::truncate;

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error>;
}

pub const ENDPOINT: &str = "https://api.elevenlabs.io/v1/speech-to-text";

/// ElevenLabs Scribe, batch endpoint. One POST per utterance: we only know the
/// audio is complete when the key comes up, so there is nothing to stream.
pub struct ScribeTranscriber {
    http: Client,
    api_key: String,
    model_id: String,
}

impl ScribeTranscriber {
    pub fn new(api_key: impl Into<String>, model_id: impl Into<String>, timeout: Duration) -> Result<Self, Error> {
        Ok(Self {
            http: Client::builder().timeout(timeout).build()?,
            api_key: api_key.into(),
            model_id: model_id.into(),
        })
    }

    /// The multipart fields other than the audio, in send order. Separate so
    /// FR-10.3/10.4 are testable without a network call.
    pub fn form_fields(model_id: &str, language: LanguageMode) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("model_id", model_id.to_owned()),
            // Off deliberately: "(laughter)" and speaker labels are noise when the
            // output is going straight into a text field.
            ("tag_audio_events", "false".to_owned()),
            ("diarize", "false".to_owned()),
        ];

        if let Some(code) = language.to_language_code() {
            fields.push(("language_code", code.to_owned()));
        }

        fields
    }

    pub fn parse(json: &str) -> Result<Transcript, Error> {
        let root: Value = serde_json::from_str(json)
            .map_err(|e| Error::new(format!("Scribe response was not JSON ({e}): {}", truncate(json, 300))))?;

        let text = match root.get("text") {
            Some(Value::String(text)) => text.trim().to_owned(),
            Some(Value::Null) => String::new(),
            _ => {
                return Err(Error::new(format!("Scribe response had no 'text' field: {}", truncate(json, 300))));
            }
        };

        let language_code = root.get("language_code").and_then(Value::as_str).map(str::to_owned);

        Ok(Transcript { text, language_code })
    }
}

impl Transcriber for ScribeTranscriber {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        let audio = Part::bytes(wav.to_vec()).file_name("utterance.wav").mime_str("audio/wav")?;

        let mut form = Form::new().part("file", audio);
        for (name, value) in Self::form_fields(&self.model_id, language) {
            form = form.text(name, value);
        }

        let response = self
            .http
            .post(ENDPOINT)
            .header("xi-api-key", self.api_key.as_str())
            .multipart(form)
            .send()?;

        let status = response.status();
        let body = response.text()?;

        if !status.is_success() {
            // The body carries the useful part — an invalid model_id comes back
            // as a 422 listing the values it will accept.
            return Err(Error::new(format!(
                "Scribe returned {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or(""),
                truncate(&body, 500)
            )));
        }

        Self::parse(&body)
    }
}
