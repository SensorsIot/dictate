use std::sync::{Arc, Mutex};

use dictate_core::{
    Cleaner, DictateConfig, DictationPipeline, Error, LanguageMode, TargetContext, Transcriber, Transcript,
    UtteranceStatus,
};

/// 0.1 s of audio with a real signal in it, so the silence guard passes.
fn some_audio() -> Vec<u8> {
    // Alternating ±8000: crude, but well above the silence threshold.
    (0..1600).flat_map(|i| (if i % 2 == 0 { 8000i16 } else { -8000 }).to_le_bytes()).collect()
}

struct StubTranscriber {
    result: Result<Transcript, Error>,
    seen_language: Arc<Mutex<Option<LanguageMode>>>,
}

impl StubTranscriber {
    fn ok(text: &str) -> Self {
        Self { result: Ok(Transcript::new(text, Some("en"))), seen_language: Arc::default() }
    }

    fn failing(message: &str) -> Self {
        Self { result: Err(Error::new(message)), seen_language: Arc::default() }
    }
}

impl Transcriber for StubTranscriber {
    fn transcribe(&self, _wav: &[u8], language: LanguageMode) -> Result<Transcript, Error> {
        *self.seen_language.lock().unwrap() = Some(language);
        self.result.clone()
    }
}

struct StubCleaner {
    result: Result<String, Error>,
    seen_target: Arc<Mutex<Option<TargetContext>>>,
}

impl StubCleaner {
    fn ok(text: &str) -> Self {
        Self { result: Ok(text.to_owned()), seen_target: Arc::default() }
    }

    fn failing(message: &str) -> Self {
        Self { result: Err(Error::new(message)), seen_target: Arc::default() }
    }
}

impl Cleaner for StubCleaner {
    fn clean(&self, _transcript: &str, target: &TargetContext) -> Result<String, Error> {
        *self.seen_target.lock().unwrap() = Some(target.clone());
        self.result.clone()
    }
}

fn build(t: StubTranscriber, c: StubCleaner) -> DictationPipeline {
    build_with(t, c, DictateConfig::default())
}

fn build_with(t: StubTranscriber, c: StubCleaner, config: DictateConfig) -> DictationPipeline {
    DictationPipeline::new(Box::new(t), Box::new(c), config)
}

#[test]
fn happy_path_delivers_the_cleaned_text() {
    let pipeline = build(StubTranscriber::ok("um so the thing is"), StubCleaner::ok("So the thing is."));

    let result = pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::Ok);
    assert_eq!(result.text, "So the thing is.");
    assert_eq!(result.raw_text, "um so the thing is");
    assert_eq!(result.error, None);
}

#[test]
fn cleanup_failure_falls_back_to_the_raw_transcript() {
    // The agreed degradation: a scruffy sentence beats a lost one.
    let pipeline = build(StubTranscriber::ok("um so the thing is"), StubCleaner::failing("503"));

    let result = pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::CleanupFailed);
    assert_eq!(result.text, "um so the thing is");
    assert!(result.error.unwrap().contains("Cleanup failed"));
}

#[test]
fn cleanup_returning_nothing_is_treated_as_a_failure_not_a_deletion() {
    let pipeline = build(StubTranscriber::ok("keep me"), StubCleaner::ok(""));

    let result = pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::CleanupFailed);
    assert_eq!(result.text, "keep me");
}

#[test]
fn swiss_spelling_applies_to_cleaned_and_fallback_text() {
    let cleaned = build(StubTranscriber::ok("die strasse"), StubCleaner::ok("Die Straße ist groß."))
        .process(&some_audio(), &TargetContext::unknown());
    let fallback = build(StubTranscriber::ok("die straße"), StubCleaner::failing("503"))
        .process(&some_audio(), &TargetContext::unknown());

    assert_eq!(cleaned.text, "Die Strasse ist gross.");
    assert_eq!(fallback.text, "die strasse");
}

#[test]
fn swiss_spelling_can_be_turned_off() {
    let config = DictateConfig { swiss_spelling: false, ..DictateConfig::default() };
    let pipeline = build_with(StubTranscriber::ok("die strasse"), StubCleaner::ok("Die Straße."), config);

    assert_eq!(pipeline.process(&some_audio(), &TargetContext::unknown()).text, "Die Straße.");
}

#[test]
fn transcription_failure_yields_no_text() {
    let pipeline = build(StubTranscriber::failing("Scribe returned 401"), StubCleaner::ok("x"));

    let result = pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::Failed);
    assert!(!result.has_text());
    assert!(result.error.unwrap().contains("401"));
}

#[test]
fn silence_is_reported_rather_than_typed() {
    let pipeline = build(StubTranscriber::ok(""), StubCleaner::ok("x"));

    let result = pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::Failed);
    assert!(!result.has_text());
}

#[test]
fn silent_audio_never_reaches_the_network_and_blames_the_microphone() {
    // The failure this exists for: a USB interface that returns well-formed
    // frames of digital silence before its capture stream has spun up.
    let transcriber = StubTranscriber::failing("must not be called");
    let seen = Arc::clone(&transcriber.seen_language);
    let pipeline = build(transcriber, StubCleaner::ok("x"));

    let result = pipeline.process(&vec![0u8; 32_000], &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::Failed);
    let error = result.error.unwrap().to_lowercase();
    assert!(error.contains("microphone"));
    assert!(!error.contains("recognised"));
    assert_eq!(*seen.lock().unwrap(), None);
}

#[test]
fn the_silence_threshold_is_configurable() {
    let config = DictateConfig { silence_threshold: 1, ..DictateConfig::default() };
    let pipeline = build_with(StubTranscriber::ok("heard it"), StubCleaner::ok("Heard it."), config);

    assert_eq!(pipeline.process(&some_audio(), &TargetContext::unknown()).status, UtteranceStatus::Ok);
}

#[test]
fn empty_audio_never_reaches_the_network() {
    let transcriber = StubTranscriber::failing("must not be called");
    let seen = Arc::clone(&transcriber.seen_language);
    let pipeline = build(transcriber, StubCleaner::ok("x"));

    let result = pipeline.process(&[], &TargetContext::unknown());

    assert_eq!(result.status, UtteranceStatus::Failed);
    assert!(result.error.unwrap().contains("No audio"));
    assert_eq!(*seen.lock().unwrap(), None);
}

#[test]
fn console_targets_get_single_line_output() {
    let pipeline = build(StubTranscriber::ok("raw"), StubCleaner::ok("line one\nline two"));

    let result = pipeline.process(&some_audio(), &TargetContext::new("wezterm-gui", "dev-1", true));

    assert_eq!(result.text, "line one line two");
}

#[test]
fn the_pinned_language_reaches_the_transcriber() {
    let transcriber = StubTranscriber::ok("raw text");
    let seen = Arc::clone(&transcriber.seen_language);
    let config = DictateConfig { language: LanguageMode::German, ..DictateConfig::default() };
    let pipeline = build_with(transcriber, StubCleaner::ok("x"), config);

    pipeline.process(&some_audio(), &TargetContext::unknown());

    assert_eq!(*seen.lock().unwrap(), Some(LanguageMode::German));
}

#[test]
fn the_focused_window_reaches_the_cleaner() {
    let cleaner = StubCleaner::ok("cleaned text");
    let seen = Arc::clone(&cleaner.seen_target);
    let target = TargetContext::new("Code", "Program.cs", false);
    let pipeline = build(StubTranscriber::ok("raw text"), cleaner);

    pipeline.process(&some_audio(), &target);

    assert_eq!(*seen.lock().unwrap(), Some(target));
}
