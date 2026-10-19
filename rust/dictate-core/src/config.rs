//! Everything the user can change, loaded from `%APPDATA%\dictate\config.json`.
//! Missing file means defaults; a broken file is a hard error, because silently
//! falling back to defaults would look like the settings simply did not take.
//!
//! Reads the same files the C# build wrote: PascalCase keys matched
//! case-insensitively, comments and trailing commas allowed, enum names in any
//! case.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::error::Error;
use crate::model::{LanguageMode, OverlayPosition};

/// Field order is the order `save` writes, kept identical to the C# class.
/// Rationale for each default lives on `DictateConfig.cs` and FSD Appendix A.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DictateConfig {
    /// Virtual-key code held to dictate. Default 0x5C = VK_RWIN. Right Alt is
    /// AltGr on a Swiss layout, so it is deliberately not the default.
    pub hotkey_virtual_key: i32,
    /// Swallow the hotkey. Required for the Windows key, which otherwise opens
    /// the Start menu on every dictation (FR-8.4).
    pub suppress_hotkey: bool,
    /// Ignore synthesised hotkey presses. Off: remote-desktop tools deliver
    /// genuine keystrokes as synthetic input too.
    pub ignore_injected_hotkey: bool,
    pub minimum_hold_ms: i32,
    pub maximum_recording_seconds: i32,
    /// Peak amplitude (0–32767) below which a recording counts as silence.
    pub silence_threshold: i32,
    pub keep_microphone_open: bool,
    /// Record from the first input whose name contains this text, e.g.
    /// "BRIO". None, or no match, means the Windows default input (FR-9.6).
    pub input_device: Option<String>,
    pub append_space_after_insert: bool,
    pub always_copy_to_clipboard: bool,
    /// Never the dictated text — see `DiagnosticLog`. Off: the user chose zero persistence.
    pub enable_diagnostic_log: bool,
    pub language: LanguageMode,
    /// Write German the Swiss way: ss, never ß (FR-11.11).
    pub swiss_spelling: bool,
    pub scribe_model_id: String,
    pub cleanup_model: String,
    pub scribe_timeout_seconds: i32,
    pub cleanup_timeout_seconds: i32,
    /// Terms Scribe reliably gets wrong. Keep it short — it rides in every request.
    pub vocabulary: Vec<String>,
    pub extra_cleanup_instruction: Option<String>,
    pub extra_console_processes: Vec<String>,
    pub injection_chunk_size: i32,
    pub injection_chunk_delay_ms: i32,
    pub show_overlay: bool,
    pub overlay_position: OverlayPosition,
}

impl Default for DictateConfig {
    fn default() -> Self {
        Self {
            hotkey_virtual_key: 0x5C,
            suppress_hotkey: true,
            ignore_injected_hotkey: false,
            minimum_hold_ms: 300,
            maximum_recording_seconds: 120,
            silence_threshold: 200,
            keep_microphone_open: false,
            input_device: None,
            append_space_after_insert: true,
            always_copy_to_clipboard: true,
            enable_diagnostic_log: false,
            language: LanguageMode::Auto,
            swiss_spelling: true,
            scribe_model_id: "scribe_v2".into(),
            cleanup_model: "claude-haiku-4-5".into(),
            scribe_timeout_seconds: 15,
            cleanup_timeout_seconds: 10,
            vocabulary: [
                "AREDN", "ESP-IDF", "ESP32", "MQTT", "Infisical", "Modbus",
                "SUN2000", "Nulleinspeisung", "devcontainer", "WezTerm",
            ]
            .map(String::from)
            .to_vec(),
            extra_cleanup_instruction: None,
            extra_console_processes: Vec::new(),
            injection_chunk_size: 200,
            injection_chunk_delay_ms: 0,
            show_overlay: true,
            overlay_position: OverlayPosition::BottomRight,
        }
    }
}

impl DictateConfig {
    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path).map_err(|e| Error::new(format!("{}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| Error::new(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let value: Value = json5::from_str(text).map_err(|e| Error::new(e.to_string()))?;

        let Value::Object(entries) = value else {
            return Err(Error::new("expected a JSON object at the top level"));
        };

        // The canonical spellings come from the struct itself, so a new field
        // cannot be forgotten here.
        let canonical: Vec<String> = match serde_json::to_value(Self::default()) {
            Ok(Value::Object(defaults)) => defaults.keys().cloned().collect(),
            _ => Vec::new(),
        };

        let mut normalised = Map::new();
        for (key, value) in entries {
            let key = canonical.iter().find(|c| c.eq_ignore_ascii_case(&key)).cloned().unwrap_or(key);
            normalised.insert(key, integral(value));
        }

        serde_json::from_value(Value::Object(normalised)).map_err(|e| Error::new(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), Error> {
        if let Some(directory) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(directory)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(|e| Error::new(e.to_string()))?;
        fs::write(path, json)?;
        Ok(())
    }
}

/// JSON5 has one number type, and a parser may hand back `300` as `300.0`,
/// which would then fail to deserialise into an integer field.
fn integral(value: Value) -> Value {
    match value {
        Value::Number(n) if n.is_f64() => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Value::Number(Number::from(f as i64)),
            _ => Value::Number(n),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(integral).collect()),
        other => other,
    }
}
