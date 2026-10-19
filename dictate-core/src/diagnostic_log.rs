//! An opt-in diagnostic log: lifecycle events, timings and outcomes.
//!
//! **It never records what was dictated.** FR-17.1 says no transcript reaches
//! disk, and that holds here — the API takes a short event name plus named
//! fields, so there is no path by which utterance text can be passed in by
//! accident. Where the size of an utterance matters, log its length.
//!
//! Off by default: the user chose zero persistence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::Error;
use crate::model::{LanguageMode, OverlayPosition, UtteranceStatus};
use crate::truncate;

/// A value that can appear in a log field.
pub trait LogValue {
    fn render(&self) -> String;
}

/// One `key=value` pair, rendered when it is built.
pub struct Field {
    key: &'static str,
    value: String,
}

pub fn field(key: &'static str, value: impl LogValue) -> Field {
    Field { key, value: value.render() }
}

pub struct DiagnosticLog {
    writer: Mutex<Option<Box<dyn Write + Send>>>,
}

impl DiagnosticLog {
    /// A log that discards everything. The default.
    pub fn disabled() -> Self {
        Self { writer: Mutex::new(None) }
    }

    /// For tests: writes to the supplied writer.
    pub fn to(writer: impl Write + Send + 'static) -> Self {
        Self { writer: Mutex::new(Some(Box::new(writer))) }
    }

    /// Opens `path` for appending, deleting it first if it has grown past
    /// `max_bytes`. A diagnostic log that fills a disk is a worse bug than the
    /// one it was added to find.
    pub fn open_file(path: &Path, max_bytes: u64) -> Result<Self, Error> {
        if let Some(directory) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(directory)?;
        }

        if fs::metadata(path).map(|m| m.len() > max_bytes).unwrap_or(false) {
            fs::remove_file(path)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self::to(file))
    }

    pub fn is_enabled(&self) -> bool {
        self.writer.lock().map(|w| w.is_some()).unwrap_or(false)
    }

    /// Writes one event. Newlines in values are flattened so a multi-line API
    /// error cannot forge extra log lines.
    pub fn event(&self, name: &str, fields: &[Field]) {
        let Ok(mut guard) = self.writer.lock() else { return };
        let Some(writer) = guard.as_mut() else { return };

        let mut line = format!("{}  {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), name);
        for f in fields {
            line.push_str("  ");
            line.push_str(f.key);
            line.push('=');
            line.push_str(&f.value);
        }

        // A log write that fails must never take dictation down with it.
        let _ = writeln!(writer, "{line}").and_then(|()| writer.flush());
    }
}

impl Default for DiagnosticLog {
    fn default() -> Self {
        Self::disabled()
    }
}

fn sanitise(text: &str) -> String {
    let flattened = text.replace(['\r', '\n'], " ");
    let flattened = flattened.trim();
    if flattened.is_empty() {
        "-".to_owned()
    } else {
        truncate(flattened, 300)
    }
}

impl LogValue for str {
    fn render(&self) -> String {
        sanitise(self)
    }
}

impl LogValue for String {
    fn render(&self) -> String {
        sanitise(self)
    }
}

impl<T: LogValue + ?Sized> LogValue for &T {
    fn render(&self) -> String {
        (**self).render()
    }
}

impl<T: LogValue> LogValue for Option<T> {
    fn render(&self) -> String {
        match self {
            Some(value) => value.render(),
            None => "-".to_owned(),
        }
    }
}

impl LogValue for Duration {
    fn render(&self) -> String {
        format!("{:.0}ms", self.as_secs_f64() * 1000.0)
    }
}

impl LogValue for f64 {
    /// Up to three decimals, trailing zeros dropped: 0.25, not 0.250.
    fn render(&self) -> String {
        let fixed = format!("{self:.3}");
        fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

impl LogValue for bool {
    fn render(&self) -> String {
        if *self { "True" } else { "False" }.to_owned()
    }
}

macro_rules! display_log_value {
    ($($type:ty),+) => {
        $(impl LogValue for $type {
            fn render(&self) -> String {
                self.to_string()
            }
        })+
    };
}

display_log_value!(i32, i64, u16, u32, u64, usize, UtteranceStatus, LanguageMode, OverlayPosition);
