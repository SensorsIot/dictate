//! Infomaniak AI Services: transcription (Whisper) and cleanup (an
//! OpenAI-compatible chat API), both hosted in Switzerland. One token covers
//! both, so the two share an `InfomaniakAccount`.
//!
//! The API is split across versions: chat lives under `/2/…/openai/v1`, while
//! transcription and its results exist only under `/1/` (there is no v2
//! audio route). The two versions also format errors differently; `Error`
//! looks for the message in both shapes.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use crate::cleaner::Cleaner;
use crate::cleanup_prompt;
use crate::config::DictateConfig;
use crate::error::{Error, ErrorKind, Service};
use crate::model::{LanguageMode, TargetContext, Transcript};
use crate::scribe::Transcriber;
use crate::truncate;

pub const API: &str = "https://api.infomaniak.com";

/// The token and the AI Services product it belongs to.
pub struct InfomaniakAccount {
    http: Client,
    token: String,
    // Configured, or looked up once on first use — on a worker thread, never
    // at startup, so a slow lookup cannot delay the tray appearing.
    product_id: Mutex<Option<u64>>,
}

impl InfomaniakAccount {
    pub fn new(token: impl Into<String>, product_id: Option<u64>) -> Result<Arc<Self>, Error> {
        Ok(Arc::new(Self {
            http: Client::builder().build()?,
            token: token.into(),
            product_id: Mutex::new(product_id),
        }))
    }

    fn product_id(&self) -> Result<u64, Error> {
        let mut cached = self.product_id.lock().map_err(|_| Error::new("Infomaniak account lock poisoned"))?;
        if let Some(id) = *cached {
            return Ok(id);
        }

        let response = self
            .http
            .get(format!("{API}/1/ai"))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(10))
            .send();
        let id = Self::parse_product_id(&checked(response)?)?;
        *cached = Some(id);
        Ok(id)
    }

    /// The first product in a `GET /1/ai` listing.
    pub fn parse_product_id(json: &str) -> Result<u64, Error> {
        let root = parse_json(json, "Infomaniak product list")?;
        root.pointer("/data/0/product_id").and_then(Value::as_u64).ok_or_else(|| {
            Error::new(format!(
                "This Infomaniak token has no AI Services product. Create one in the Infomaniak Manager, \
                 or set InfomaniakProductId. Response: {}",
                truncate(json, 300)
            ))
        })
    }
}

// ---- Transcription ----------------------------------------------------------

/// What a result poll found.
#[derive(Debug, PartialEq, Eq)]
pub enum BatchState {
    Running,
    Done(Transcript),
}

pub struct InfomaniakTranscriber {
    account: Arc<InfomaniakAccount>,
    model: String,
    vocabulary: Vec<String>,
    /// Upload plus every poll must finish inside this.
    deadline: Duration,
}

impl InfomaniakTranscriber {
    pub fn new(account: Arc<InfomaniakAccount>, config: &DictateConfig) -> Self {
        Self {
            account,
            model: config.effective_transcription_model().to_owned(),
            vocabulary: config.vocabulary.clone(),
            deadline: Duration::from_secs(config.scribe_timeout_seconds.max(1) as u64),
        }
    }

    /// The multipart fields other than the audio.
    ///
    /// A pinned language is sent: despite the API reference saying a language
    /// "will translate the output", German audio sent with `en` came back in
    /// German. The vocabulary rides as Whisper's prompt, which biases spelling.
    /// `verbose_json` because only it reports the detected language, which the
    /// diagnostic log records so a misdetection can be seen.
    pub fn form_fields(model: &str, language: LanguageMode, vocabulary: &[String]) -> Vec<(&'static str, String)> {
        let mut fields = vec![("model", model.to_owned()), ("response_format", "verbose_json".to_owned())];
        if let Some(code) = language.to_language_code() {
            fields.push(("language", code.to_owned()));
        }
        if !vocabulary.is_empty() {
            fields.push(("prompt", vocabulary.join(", ")));
        }
        fields
    }

    /// The submit response: `{"batch_id": …}`, possibly inside the v1 envelope.
    pub fn parse_batch_id(json: &str) -> Result<String, Error> {
        let root = parse_json(json, "Infomaniak transcription")?;
        unwrap_envelope(&root)
            .get("batch_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::new(format!("Infomaniak returned no batch id: {}", truncate(json, 300))))
    }

    /// A `GET /1/ai/{product}/results/{batch}` response. Its `data` is the
    /// Whisper `verbose_json` document, delivered as a JSON string.
    pub fn parse_result(json: &str) -> Result<BatchState, Error> {
        let root = parse_json(json, "Infomaniak result")?;
        let result = unwrap_envelope(&root);
        match result.get("status").and_then(Value::as_str) {
            Some("pending" | "processing") => Ok(BatchState::Running),
            Some("success") => {
                let document = match result.get("data") {
                    Some(Value::String(inner)) => parse_json(inner, "Infomaniak transcript")?,
                    Some(inner) => inner.clone(),
                    None => Value::Null,
                };
                let text = document.get("text").and_then(Value::as_str).unwrap_or_default().trim();
                let language = document.get("language").and_then(Value::as_str);
                Ok(BatchState::Done(Transcript::new(text, language)))
            }
            _ => Err(Error::new(format!("Infomaniak transcription failed: {}", truncate(json, 500)))),
        }
    }
}

impl Transcriber for InfomaniakTranscriber {
    fn transcribe(&self, wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        let started = Instant::now();
        let remaining = || self.deadline.saturating_sub(started.elapsed());
        let product = self.account.product_id()?;

        let audio = Part::bytes(wav.to_vec()).file_name("utterance.wav").mime_str("audio/wav")?;
        let mut form = Form::new().part("file", audio);
        for (name, value) in Self::form_fields(&self.model, language, &self.vocabulary) {
            form = form.text(name, value);
        }

        let response = self
            .account
            .http
            .post(format!("{API}/1/ai/{product}/openai/audio/transcriptions"))
            .bearer_auth(&self.account.token)
            .multipart(form)
            .timeout(remaining())
            .send();
        let batch = Self::parse_batch_id(&checked(response)?)?;

        // Each poll takes about a second on Infomaniak's side, so the pause
        // between them is short: it only keeps a fast failure from spinning.
        loop {
            if remaining().is_zero() {
                return Err(Error::of(
                    Service::Infomaniak,
                    ErrorKind::TimedOut,
                    format!("Infomaniak did not finish transcribing within {} s", self.deadline.as_secs()),
                ));
            }
            let response = self
                .account
                .http
                .get(format!("{API}/1/ai/{product}/results/{batch}"))
                .bearer_auth(&self.account.token)
                .timeout(remaining())
                .send();
            match Self::parse_result(&checked(response)?)? {
                BatchState::Done(transcript) => return Ok(transcript),
                BatchState::Running => thread::sleep(Duration::from_millis(100).min(remaining())),
            }
        }
    }
}

// ---- Cleanup ----------------------------------------------------------------

pub struct InfomaniakCleaner {
    account: Arc<InfomaniakAccount>,
    model: String,
    system_prompt: String,
    timeout: Duration,
}

impl InfomaniakCleaner {
    pub fn new(account: Arc<InfomaniakAccount>, config: &DictateConfig) -> Self {
        Self {
            account,
            model: config.effective_cleanup_model().to_owned(),
            system_prompt: cleanup_prompt::build_system(config),
            timeout: Duration::from_secs(config.cleanup_timeout_seconds.max(1) as u64),
        }
    }

    pub fn request_body(model: &str, system_prompt: &str, transcript: &str, target: &TargetContext) -> Value {
        json!({
            "model": model,
            "max_completion_tokens": 2048,
            // FR-11.8.
            "temperature": 0,
            // Both default to on at Infomaniak; either would add latency for
            // nothing — one finished answer is all cleanup needs.
            "stream": false,
            "reasoning_effort": "none",
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": cleanup_prompt::build_user(transcript, target) }
            ]
        })
    }

    /// The first choice's message content. A separate reasoning field, if a
    /// model sends one, is the model thinking and never text to type.
    pub fn extract_text(json: &str) -> Result<String, Error> {
        let root = parse_json(json, "Cleanup response")?;
        root.pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(|text| text.trim().to_owned())
            .ok_or_else(|| Error::new(format!("Cleanup response had no content: {}", truncate(json, 300))))
    }
}

impl Cleaner for InfomaniakCleaner {
    fn clean(&self, transcript: &str, target: &TargetContext) -> Result<String, Error> {
        let product = self.account.product_id()?;
        let body = Self::request_body(&self.model, &self.system_prompt, transcript, target);

        let response = self
            .account
            .http
            .post(format!("{API}/2/ai/{product}/openai/v1/chat/completions"))
            .bearer_auth(&self.account.token)
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .timeout(self.timeout)
            .send();

        Self::extract_text(&checked(response)?)
    }
}

// ---- Shared -----------------------------------------------------------------

/// The body of a successful response; otherwise an error sorted by what the
/// user can do about it, carrying the API's own words (FR-7.4, FR-7.5).
fn checked(response: reqwest::Result<reqwest::blocking::Response>) -> Result<String, Error> {
    let response = response.map_err(|e| Error::transport(Service::Infomaniak, e))?;
    let status = response.status();
    let text = response.text().map_err(|e| Error::transport(Service::Infomaniak, e))?;
    if status.is_success() {
        Ok(text)
    } else {
        Err(Error::from_response(Service::Infomaniak, status.as_u16(), &text))
    }
}

fn parse_json(json: &str, what: &str) -> Result<Value, Error> {
    serde_json::from_str(json).map_err(|e| Error::new(format!("{what} was not JSON ({e}): {}", truncate(json, 300))))
}

/// v1 routes wrap their payload as `{"result": "success", "data": {…}}`.
fn unwrap_envelope(root: &Value) -> &Value {
    match root.get("data") {
        Some(inner) if inner.is_object() && root.get("result").is_some() => inner,
        _ => root,
    }
}
