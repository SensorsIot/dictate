use dictate_core::infomaniak::BatchState;
use dictate_core::{
    providers, CleanupProvider, Credential, DictateConfig, InfomaniakAccount, InfomaniakCleaner, InfomaniakTranscriber,
    LanguageMode, TargetContext, TranscriptionProvider,
};

// ---- Transcription (H-21) ---------------------------------------------------

#[test]
fn the_transcription_request_carries_model_language_and_vocabulary() {
    let vocabulary = vec!["AREDN".to_owned(), "ESP32".to_owned()];

    let pinned = InfomaniakTranscriber::form_fields("whisper", LanguageMode::German, &vocabulary);
    assert!(pinned.contains(&("model", "whisper".to_owned())));
    assert!(pinned.contains(&("response_format", "text".to_owned())));
    assert!(pinned.contains(&("language", "de".to_owned())));
    assert!(pinned.contains(&("prompt", "AREDN, ESP32".to_owned())));

    let auto = InfomaniakTranscriber::form_fields("whisper", LanguageMode::Auto, &[]);
    assert!(auto.iter().all(|(name, _)| *name != "language"), "Auto lets Whisper detect");
    assert!(auto.iter().all(|(name, _)| *name != "prompt"), "no empty prompt");
}

#[test]
fn the_batch_id_is_read_with_or_without_the_v1_envelope() {
    assert_eq!(InfomaniakTranscriber::parse_batch_id(r#"{"batch_id":"abc-1"}"#).unwrap(), "abc-1");
    assert_eq!(
        InfomaniakTranscriber::parse_batch_id(r#"{"result":"success","data":{"batch_id":"abc-2"}}"#).unwrap(),
        "abc-2"
    );
    assert!(InfomaniakTranscriber::parse_batch_id(r#"{"result":"error","error":{"code":"not_authorized"}}"#).is_err());
}

#[test]
fn a_result_poll_distinguishes_running_done_and_failed() {
    let running = r#"{"result":"success","data":{"status":"processing","data":null}}"#;
    assert_eq!(InfomaniakTranscriber::parse_result(running).unwrap(), BatchState::Running);

    let done = r#"{"result":"success","data":{"status":"success","data":"Das ist ein kurzer Test.\n"}}"#;
    assert_eq!(InfomaniakTranscriber::parse_result(done).unwrap(), BatchState::Done("Das ist ein kurzer Test.".into()));

    // A failed batch must surface as an error, never as an empty transcript.
    let failed = r#"{"result":"success","data":{"status":"failed","data":null}}"#;
    assert!(InfomaniakTranscriber::parse_result(failed).is_err());
}

#[test]
fn the_product_is_found_from_the_token() {
    let listing = r#"{"result":"success","data":[{"product_name":"Ai-Tools","product_id":110634,"status":"ok"}]}"#;
    assert_eq!(InfomaniakAccount::parse_product_id(listing).unwrap(), 110634);
    assert!(InfomaniakAccount::parse_product_id(r#"{"result":"success","data":[]}"#).is_err());
}

// ---- Cleanup (H-22) ---------------------------------------------------------

#[test]
fn the_cleanup_request_is_deterministic_unstreamed_and_unreasoned() {
    // FR-11.8. Infomaniak streams and reasons by default; both would add latency.
    let body = InfomaniakCleaner::request_body("google/gemma-4-31B-it", "system", "hello", &TargetContext::unknown());

    assert_eq!(body["temperature"], 0);
    assert_eq!(body["stream"], false);
    assert_eq!(body["reasoning_effort"], "none");
    assert_eq!(body["model"], "google/gemma-4-31B-it");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body["messages"][1]["content"].as_str().unwrap().contains("hello"));
}

#[test]
fn only_the_message_content_is_typed() {
    let text = InfomaniakCleaner::extract_text(
        r#"{"choices":[{"message":{"role":"assistant","content":"\n\nHallo Welt. ","reasoning":"The user wants"}}]}"#,
    )
    .unwrap();
    assert_eq!(text, "Hallo Welt.");
    assert!(InfomaniakCleaner::extract_text(r#"{"error":{"message":"Invalid Authentication"}}"#).is_err());
}

// ---- Provider selection (H-23) ----------------------------------------------

#[test]
fn models_follow_the_provider_unless_set() {
    let defaults = DictateConfig::default();
    assert_eq!(defaults.effective_transcription_model(), "scribe_v2");
    assert_eq!(defaults.effective_cleanup_model(), "claude-haiku-4-5");

    let swiss = DictateConfig::parse(r#"{ "transcriptionprovider": "infomaniak", "CleanupProvider": "Infomaniak" }"#).unwrap();
    assert_eq!(swiss.transcription_provider, TranscriptionProvider::Infomaniak);
    assert_eq!(swiss.cleanup_provider, CleanupProvider::Infomaniak);
    assert_eq!(swiss.effective_transcription_model(), "whisper");
    assert_eq!(swiss.effective_cleanup_model(), "google/gemma-4-31B-it");

    let pinned = DictateConfig::parse(
        r#"{ "CleanupProvider": "Infomaniak", "CleanupModel": "mistralai/Mistral-Small-4-119B-2603" }"#,
    )
    .unwrap();
    assert_eq!(pinned.effective_cleanup_model(), "mistralai/Mistral-Small-4-119B-2603");

    // Files from before providers existed keep their Scribe model.
    let legacy = DictateConfig::parse(r#"{ "ScribeModelId": "scribe_v3" }"#).unwrap();
    assert_eq!(legacy.effective_transcription_model(), "scribe_v3");

    assert!(DictateConfig::parse(r#"{ "CleanupProvider": "OpenAI" }"#).is_err());
}

#[test]
fn each_provider_reads_its_own_key() {
    let config = DictateConfig::parse(r#"{ "TranscriptionProvider": "Infomaniak", "CleanupProvider": "Anthropic" }"#)
        .unwrap();

    // The transcription key is required…
    let missing = providers::build(&config, |c| (c == Credential::Anthropic).then(|| "a".to_owned()));
    assert!(missing.is_err());
    assert!(missing.err().unwrap().message().contains("Infomaniak"));

    // …the cleanup key is not (FR-11.10), and the ElevenLabs key is never asked for.
    let asked = std::cell::RefCell::new(Vec::new());
    let built = providers::build(&config, |c| {
        asked.borrow_mut().push(c);
        (c == Credential::Infomaniak).then(|| "i".to_owned())
    });
    assert!(built.is_ok());
    assert!(!asked.borrow().contains(&Credential::ElevenLabs));
}
