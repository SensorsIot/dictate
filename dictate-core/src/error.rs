use std::fmt;

use serde_json::Value;

use crate::truncate;

/// A remote service dictate pays for. Named in notifications, because whose
/// outage or whose account it is decides what the user does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    ElevenLabs,
    Anthropic,
    Infomaniak,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Service::ElevenLabs => "ElevenLabs",
            Service::Anthropic => "Anthropic",
            Service::Infomaniak => "Infomaniak",
        }
    }

    /// Where the user buys more, as an instruction.
    pub fn top_up(self) -> &'static str {
        match self {
            Service::ElevenLabs => "Top up or upgrade at elevenlabs.io → Subscription, or wait for the monthly reset.",
            Service::Anthropic => "Buy credit at platform.claude.com/spending.",
            Service::Infomaniak => "Top up AI Tools in the Infomaniak Manager.",
        }
    }
}

/// A failure sorted by what the user can do about it (FR-7.5). The HTTP
/// status alone does not decide it: ElevenLabs reports an exhausted quota as
/// 401 and an outage as 429, Anthropic an empty balance as 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The service is overloaded. Waiting is the only fix.
    Overloaded,
    /// The service failed internally. Waiting is the only fix.
    ServerFault,
    /// The account sent more requests than its plan allows.
    RateLimited,
    /// The account has no credit left.
    OutOfCredit,
    /// The key is wrong, revoked, or lacks permission.
    KeyRejected,
    /// The service refused the request itself — in practice a model name.
    Rejected,
    /// No connection to the service.
    Unreachable,
    /// No answer within the configured timeout.
    TimedOut,
    Other,
}

/// A failure: the full story for the diagnostic log, plus what the
/// notification needs — whose failure, which kind, and the service's own
/// words (FR-7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
    kind: ErrorKind,
    service: Option<Service>,
    said: Option<String>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), kind: ErrorKind::Other, service: None, said: None }
    }

    pub fn of(service: Service, kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { message: message.into(), kind, service: Some(service), said: None }
    }

    /// A non-success HTTP response.
    pub fn from_response(service: Service, status: u16, body: &str) -> Self {
        let reason = reqwest::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("");
        Self {
            message: format!("{} returned {status} {reason}: {}", service.name(), truncate(body, 500)),
            kind: classify(status, body),
            service: Some(service),
            said: service_message(body),
        }
    }

    /// A request that never produced a response.
    pub fn transport(service: Service, error: reqwest::Error) -> Self {
        let kind = if error.is_timeout() {
            ErrorKind::TimedOut
        } else if error.is_connect() || error.is_request() {
            ErrorKind::Unreachable
        } else {
            ErrorKind::Other
        };
        Self { message: format!("{}: {}", service.name(), Self::from(error).message), kind, service: Some(service), said: None }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn service(&self) -> Option<Service> {
        self.service
    }

    /// The service's own explanation, when its response carried one.
    pub fn said(&self) -> Option<&str> {
        self.said.as_deref()
    }
}

fn classify(status: u16, body: &str) -> ErrorKind {
    let body = body.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| body.contains(word));

    if status == 402 || says(&["quota_exceeded", "insufficient_quota", "credit balance", "usage limit", "payment_required"]) {
        return ErrorKind::OutOfCredit;
    }
    match status {
        401 | 403 => ErrorKind::KeyRejected,
        429 if says(&["system_busy", "overloaded"]) => ErrorKind::Overloaded,
        429 => ErrorKind::RateLimited,
        503 | 529 => ErrorKind::Overloaded,
        500..=599 => ErrorKind::ServerFault,
        400..=499 => ErrorKind::Rejected,
        _ => ErrorKind::Other,
    }
}

/// The human sentence in an error body. Each provider nests it differently:
/// ElevenLabs under `detail`, Anthropic and Infomaniak v2 under `error`,
/// Infomaniak v1 as `error.description`.
fn service_message(body: &str) -> Option<String> {
    let root: Value = serde_json::from_str(body).ok()?;
    ["/detail/message", "/error/message", "/error/description", "/message", "/detail", "/error"]
        .iter()
        .filter_map(|pointer| root.pointer(pointer).and_then(Value::as_str))
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_owned)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // reqwest's own Display is "error sending request for url (…)"; the
        // reason — timeout, DNS, TLS — is further down the chain.
        use std::error::Error as _;

        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(inner) = source {
            message.push_str(": ");
            message.push_str(&inner.to_string());
            source = inner.source();
        }
        Self::new(message)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}
