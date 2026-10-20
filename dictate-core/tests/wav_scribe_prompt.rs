use dictate_core::cleanup_prompt::{build_system, build_user};
use dictate_core::{wav, DictateConfig, HaikuCleaner, LanguageMode, PizoraCleaner, ScribeTranscriber, TargetContext};

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

// ---- WAV --------------------------------------------------------------------

#[test]
fn header_describes_16k_mono_16_bit() {
    let pcm = vec![0u8; 1000];

    let wav = wav::from_pcm16(&pcm);

    assert_eq!(wav.len(), 44 + pcm.len());
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(&wav[36..40], b"data");

    assert_eq!(u16_at(&wav, 20), 1); // PCM
    assert_eq!(u16_at(&wav, 22), 1); // mono
    assert_eq!(u32_at(&wav, 24), 16_000); // sample rate
    assert_eq!(u32_at(&wav, 28), 32_000); // byte rate
    assert_eq!(u16_at(&wav, 32), 2); // block align
    assert_eq!(u16_at(&wav, 34), 16); // bit depth
}

#[test]
fn declared_sizes_match_the_payload() {
    let wav = wav::from_pcm16(&[0u8; 1000]);

    assert_eq!(u32_at(&wav, 4), 36 + 1000);
    assert_eq!(u32_at(&wav, 40), 1000);
}

#[test]
fn audio_is_copied_verbatim() {
    let pcm = [1u8, 2, 3, 4, 250, 251];

    assert_eq!(&wav::from_pcm16(&pcm)[44..], &pcm);
}

#[test]
fn duration_is_one_second_for_one_seconds_worth_of_bytes() {
    assert!((wav::duration_of(32_000).as_secs_f64() - 1.0).abs() < 0.001);
}

// ---- Scribe -----------------------------------------------------------------

#[test]
fn text_and_language_are_extracted() {
    let transcript = ScribeTranscriber::parse(
        r#"{"language_code":"de","language_probability":0.98,"text":"Guten Morgen","words":[]}"#,
    )
    .unwrap();

    assert_eq!(transcript.text, "Guten Morgen");
    assert_eq!(transcript.language_code.as_deref(), Some("de"));
}

#[test]
fn surrounding_whitespace_is_trimmed_from_the_transcript() {
    assert_eq!(ScribeTranscriber::parse(r#"{"text":"  hello  "}"#).unwrap().text, "hello");
}

#[test]
fn a_missing_language_is_not_fatal() {
    assert_eq!(ScribeTranscriber::parse(r#"{"text":"hello"}"#).unwrap().language_code, None);
}

#[test]
fn a_response_without_text_is_an_error() {
    assert!(ScribeTranscriber::parse(r#"{"detail":"nope"}"#).is_err());
}

#[test]
fn auto_omits_language_code_and_a_pinned_language_sends_it() {
    // FR-10.3 / FR-10.4. The form is a pure function, so no HTTP is needed.
    let auto = ScribeTranscriber::form_fields("scribe_v2", LanguageMode::Auto);
    let german = ScribeTranscriber::form_fields("scribe_v2", LanguageMode::German);

    assert!(auto.iter().all(|(name, _)| *name != "language_code"));
    assert!(german.contains(&("language_code", "de".to_owned())));
    assert!(auto.contains(&("model_id", "scribe_v2".to_owned())));
}

// ---- Cleanup prompt ---------------------------------------------------------

#[test]
fn the_system_prompt_forbids_translating_and_answering() {
    let prompt = build_system(&DictateConfig::default()).to_lowercase();

    assert!(prompt.contains("translate"));
    assert!(prompt.contains("answer"));
}

#[test]
fn vocabulary_is_listed_for_the_model() {
    let mut config = DictateConfig::default();
    config.vocabulary.push("HB9XYZ".into());

    assert!(build_system(&config).contains("HB9XYZ"));
}

#[test]
fn an_empty_vocabulary_does_not_produce_a_dangling_heading() {
    let config = DictateConfig { vocabulary: Vec::new(), ..DictateConfig::default() };

    assert!(!build_system(&config).contains("Spell these terms"));
}

#[test]
fn extra_instructions_are_appended() {
    let config = DictateConfig {
        extra_cleanup_instruction: Some("Prefer British spelling.".into()),
        ..DictateConfig::default()
    };

    assert!(build_system(&config).contains("Prefer British spelling."));
}

#[test]
fn the_user_prompt_carries_the_transcript_and_the_window() {
    let prompt = build_user("hello there", &TargetContext::new("Code", "main.rs", false));

    assert!(prompt.contains("hello there"));
    assert!(prompt.contains("Code"));
    assert!(prompt.contains("main.rs"));
}

#[test]
fn the_transcript_is_delimited_and_replies_are_forbidden() {
    let prompt = build_user("ok", &TargetContext::unknown());
    let system = build_system(&DictateConfig::default());

    assert!(prompt.contains("<transcript>\nok\n</transcript>"));
    assert!(system.contains("Reply to the user, ask for input"));
}

#[test]
fn a_console_target_asks_for_a_single_line() {
    let prompt = build_user("git status", &TargetContext::new("wezterm-gui", "dev-1", true));

    assert!(prompt.contains("single line"));
}

// ---- Messages API -----------------------------------------------------------

#[test]
fn the_cleanup_request_is_deterministic_and_carries_the_model() {
    // FR-11.8: temperature 0. The request body is ours, so it is asserted.
    let body = HaikuCleaner::request_body("claude-haiku-4-5", "system", "hello", &TargetContext::unknown());

    assert_eq!(body["temperature"], 0);
    assert_eq!(body["model"], "claude-haiku-4-5");
    assert_eq!(body["system"], "system");
    assert_eq!(body["messages"][0]["role"], "user");
    assert!(body["messages"][0]["content"].as_str().unwrap().contains("hello"));
}

#[test]
fn only_text_blocks_are_taken_from_the_response() {
    let text = HaikuCleaner::extract_text(
        r#"{"content":[{"type":"text","text":" Hallo "},{"type":"thinking","thinking":"x"},{"type":"text","text":"Welt. "}]}"#,
    )
    .unwrap();

    assert_eq!(text, "Hallo Welt.");
}

#[test]
fn a_response_without_content_is_an_error() {
    assert!(HaikuCleaner::extract_text(r#"{"type":"error"}"#).is_err());
}

// ---- Pizora chat completions (H-21) -----------------------------------------

#[test]
fn the_pizora_request_is_deterministic_and_carries_the_system_prompt_first() {
    // FR-11.8, FR-11.2.
    let body = PizoraCleaner::request_body("pizora/bernina-1", "system", "hello", &TargetContext::unknown());

    assert_eq!(body["temperature"], 0);
    assert_eq!(body["model"], "pizora/bernina-1");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body["messages"][1]["content"].as_str().unwrap().contains("hello"));
    assert!(body.get("system").is_none(), "no Anthropic-style top-level system field");
}

#[test]
fn only_the_message_content_is_taken_from_a_pizora_response() {
    let text = PizoraCleaner::extract_text(
        r#"{"choices":[{"index":0,"message":{"role":"assistant","content":" Hallo Welt. ","reasoning":"The user wants"}}]}"#,
    )
    .unwrap();

    assert_eq!(text, "Hallo Welt.");
}

#[test]
fn a_pizora_response_without_content_is_an_error() {
    assert!(PizoraCleaner::extract_text(r#"{"error":{"code":"insufficient_quota"}}"#).is_err());
    assert!(PizoraCleaner::extract_text("not json").is_err());
}
