use dictate_core::{CleanupProvider, DictateConfig, LanguageMode, OverlayPosition, TranscriptionProvider};

#[test]
fn defaults_match_the_configuration_catalogue() {
    // FSD Appendix A (H-15).
    let config = DictateConfig::default();

    assert_eq!(config.hotkey_virtual_key, 0x5C);
    assert!(config.suppress_hotkey);
    assert!(!config.ignore_injected_hotkey);
    assert_eq!(config.minimum_hold_ms, 300);
    assert_eq!(config.maximum_recording_seconds, 120);
    assert_eq!(config.language, LanguageMode::Auto);
    assert!(config.swiss_spelling);
    assert_eq!(config.input_device, None);
    assert_eq!(config.scribe_model_id, "scribe_v2");
    assert_eq!(config.transcription_provider, TranscriptionProvider::ElevenLabs);
    assert_eq!(config.cleanup_provider, CleanupProvider::Anthropic);
    assert_eq!(config.cleanup_model, None);
    assert_eq!(config.infomaniak_product_id, None);
    assert_eq!(config.silence_threshold, 200);
    assert!(!config.enable_diagnostic_log);
    assert_eq!(config.overlay_position, OverlayPosition::BottomRight);
}

#[test]
fn a_missing_file_yields_the_defaults() {
    let path = std::env::temp_dir().join("dictate-core-tests-does-not-exist").join("config.json");

    assert_eq!(DictateConfig::load(&path).unwrap(), DictateConfig::default());
}

#[test]
fn a_malformed_file_is_an_error_not_a_silent_default() {
    // FR-16.2. A typo that quietly undid every setting would look exactly like
    // the settings never applying.
    assert!(DictateConfig::parse("{ \"Language\": ").is_err());
    assert!(DictateConfig::parse("null").is_err());
}

#[test]
fn hand_edited_files_are_read_forgivingly() {
    // config.json is edited by hand: keys in any case, comments, a trailing
    // comma, enum values in lower case, and keys from a newer version.
    let config = DictateConfig::parse(
        r#"{
            // hand-edited
            "hotkeyVirtualKey": 163,
            "LANGUAGE": "german",
            "SwissSpelling": false,
            "inputdevice": "BRIO",
            "OverlayPosition": "NearCursor",
            "Vocabulary": ["HB9XYZ"],
            "SomethingFromTheFuture": 1,
        }"#,
    )
    .unwrap();

    assert_eq!(config.hotkey_virtual_key, 163);
    assert_eq!(config.language, LanguageMode::German);
    assert!(!config.swiss_spelling);
    assert_eq!(config.input_device.as_deref(), Some("BRIO"));
    assert_eq!(config.overlay_position, OverlayPosition::NearCursor);
    assert_eq!(config.vocabulary, ["HB9XYZ"]);
    assert_eq!(config.minimum_hold_ms, 300, "unspecified keys keep their defaults");
}

#[test]
fn one_setting_can_be_changed_without_rewriting_the_file() {
    let directory = std::env::temp_dir().join(format!("dictate-core-set-{}", std::process::id()));
    let path = directory.join("config.json");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(&path, r#"{ "EnableDiagnosticLog": true, "inputdevice": "Old" }"#).unwrap();

    DictateConfig::set_string_in_file(&path, "InputDevice", Some("BRIO")).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let loaded = DictateConfig::load(&path).unwrap();

    DictateConfig::set_string_in_file(&path, "InputDevice", None).unwrap();
    let cleared = DictateConfig::load(&path).unwrap();
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(loaded.input_device.as_deref(), Some("BRIO"));
    assert!(loaded.enable_diagnostic_log, "other keys survive");
    assert!(!written.contains("MinimumHoldMs"), "defaults are not written out: {written}");
    assert!(!written.contains("Old"), "the old spelling of the key is replaced, not duplicated");
    assert_eq!(cleared.input_device, None);
}

#[test]
fn an_unknown_enum_value_is_an_error() {
    assert!(DictateConfig::parse(r#"{ "Language": "Klingon" }"#).is_err());
}

#[test]
fn save_and_load_round_trip() {
    let directory = std::env::temp_dir().join(format!("dictate-core-tests-{}", std::process::id()));
    let path = directory.join("config.json");
    let config = DictateConfig {
        language: LanguageMode::English,
        extra_cleanup_instruction: Some("Prefer British spelling.".into()),
        ..DictateConfig::default()
    };

    config.save(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let loaded = DictateConfig::load(&path).unwrap();
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(loaded, config);
    assert!(written.contains("\"Language\": \"English\""), "PascalCase keys, enum names: {written}");
}

#[test]
fn only_a_short_suppressed_press_is_handed_back() {
    // FR-8.9 (H-24). Must not: a replay when the key already reached the system
    // — Caps Lock would toggle twice and end up unchanged.
    use std::time::Duration;

    let config = DictateConfig { short_press_passes_through: true, ..DictateConfig::default() };
    assert!(config.replays_tap(Duration::from_millis(120)));
    assert!(!config.replays_tap(Duration::from_millis(300)), "a held key dictates");

    let unsuppressed = DictateConfig { suppress_hotkey: false, ..config.clone() };
    assert!(!unsuppressed.replays_tap(Duration::from_millis(120)));

    assert!(!DictateConfig::default().replays_tap(Duration::from_millis(120)), "off by default");
}
