//! The local runtime lives outside the tray process so its model stays loaded
//! without adding a GPU dependency to the portable Rust core.

use std::time::Duration;

use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::config::DictateConfig;
use crate::error::{Error, ErrorKind, Service};
use crate::model::{LanguageMode, Transcript};
use crate::scribe::Transcriber;

pub const MODELS: &[&str] = &[
    "tiny",
    "tiny.en",
    "base",
    "base.en",
    "small",
    "small.en",
    "medium",
    "medium.en",
    "large-v1",
    "large-v2",
    "large-v3",
    "turbo",
];

pub struct LocalWhisperTranscriber {
    http: Client,
    endpoint: String,
    model: String,
    vocabulary: Vec<String>,
}

impl LocalWhisperTranscriber {
    pub fn new(config: &DictateConfig) -> Result<Self, Error> {
        let model = config.effective_transcription_model();
        if !MODELS.contains(&model) {
            return Err(Error::new(format!(
                "Unknown local Whisper model '{model}'. Choose one of: {}.",
                MODELS.join(", ")
            )));
        }
        if config.local_whisper_port == 0 {
            return Err(Error::new("LocalWhisperPort must be between 1 and 65535."));
        }
        Ok(Self {
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(config.local_whisper_timeout_seconds.max(1) as u64))
                .build()?,
            endpoint: format!("http://127.0.0.1:{}/v1/audio/transcriptions", config.local_whisper_port),
            model: model.to_owned(),
            vocabulary: config.vocabulary.clone(),
        })
    }

    pub fn form_fields(model: &str, language: LanguageMode, vocabulary: &[String]) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("model", model.to_owned()),
            ("response_format", "verbose_json".to_owned()),
        ];
        if let Some(code) = language.to_language_code() {
            fields.push(("language", code.to_owned()));
        }
        if !vocabulary.is_empty() {
            fields.push(("prompt", vocabulary.join(", ")));
        }
        fields
    }

    pub fn parse(body: &str) -> Result<Transcript, Error> {
        // Do not include response text in parse errors: it could be a transcript
        // and errors may be recorded in the metadata-only diagnostic log.
        let root: Value = serde_json::from_str(body).map_err(|_| {
            Error::of(
                Service::LocalWhisper,
                ErrorKind::Other,
                "Local Whisper returned invalid JSON.",
            )
        })?;
        let text = root.get("text").and_then(Value::as_str).ok_or_else(|| {
            Error::of(
                Service::LocalWhisper,
                ErrorKind::Other,
                "Local Whisper response had no text field.",
            )
        })?;
        Ok(Transcript {
            text: text.trim().to_owned(),
            language_code: root.get("language").and_then(Value::as_str).map(str::to_owned),
        })
    }
}

impl Transcriber for LocalWhisperTranscriber {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        let audio = Part::bytes(wav.to_vec())
            .file_name("utterance.wav")
            .mime_str("audio/wav")?;
        let mut form = Form::new().part("file", audio);
        for (name, value) in Self::form_fields(&self.model, language, &self.vocabulary) {
            form = form.text(name, value);
        }
        let response = self
            .http
            .post(&self.endpoint)
            .multipart(form)
            .send()
            .map_err(|e| Error::transport(Service::LocalWhisper, e))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|e| Error::transport(Service::LocalWhisper, e))?;
        if !status.is_success() {
            return Err(Error::from_response(Service::LocalWhisper, status.as_u16(), &body));
        }
        Self::parse(&body)
    }
}
