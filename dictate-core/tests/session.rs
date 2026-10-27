use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dictate_core::{
    Cleaner, DiagnosticLog, DictateConfig, DictationPipeline, Error, LanguageMode, Platform, Service, Session,
    SessionState, StopReason, TargetContext, Transcriber, Transcript,
};

/// Everything the fake platform was asked to do, and the one thing a test
/// steers: which window has focus.
#[derive(Default)]
struct World {
    focused: u32,
    has_device: bool,
    typing_fails: bool,
    /// Background jobs wait in `jobs` until a test runs them with `complete`.
    defer_jobs: bool,
    jobs: Vec<Option<Box<dyn FnOnce() + Send>>>,
    captures_started: u32,
    typed: Vec<String>,
    clipboard: Vec<String>,
    notices: Vec<String>,
    details: Vec<String>,
    states: Vec<SessionState>,
}

struct FakePlatform(Rc<RefCell<World>>);

impl Platform for FakePlatform {
    type Target = u32;

    fn focused(&self) -> u32 {
        self.0.borrow().focused
    }

    fn describe(&self, target: u32, _extra: &[String]) -> TargetContext {
        TargetContext::new(format!("app{target}"), "", false)
    }

    fn has_input_device(&self) -> bool {
        self.0.borrow().has_device
    }

    fn start_capture(&mut self) {
        self.0.borrow_mut().captures_started += 1;
    }

    /// The n-th capture is n times as long, so a transcriber can tell them apart.
    fn stop_capture(&mut self) -> Vec<u8> {
        audio(self.0.borrow().captures_started as usize)
    }

    fn take_capture_error(&mut self) -> Option<String> {
        Some("device busy".into())
    }

    fn show_state(&mut self, state: SessionState) {
        self.0.borrow_mut().states.push(state);
    }

    fn notify(&self, title: &str, detail: &str) {
        self.0.borrow_mut().notices.push(title.to_owned());
        self.0.borrow_mut().details.push(detail.to_owned());
    }

    fn type_text(&self, text: &str, _config: &DictateConfig) -> Result<(), String> {
        if self.0.borrow().typing_fails {
            return Err("blocked".into());
        }
        self.0.borrow_mut().typed.push(text.to_owned());
        Ok(())
    }

    fn copy_to_clipboard(&self, text: &str) -> Result<(), String> {
        self.0.borrow_mut().clipboard.push(text.to_owned());
        Ok(())
    }

    /// Synchronous unless `defer_jobs`: by the time `end` returns, the result
    /// is waiting for `finish`.
    fn run_in_background(&self, job: Box<dyn FnOnce() + Send>) {
        if self.0.borrow().defer_jobs {
            self.0.borrow_mut().jobs.push(Some(job));
        } else {
            job();
        }
    }

    fn paste_hint(&self) -> &'static str {
        "Ctrl+V"
    }
}

struct FixedTranscriber(Result<Transcript, Error>);

impl Transcriber for FixedTranscriber {
    fn transcribe(&self, _wav: &[u8], _language: LanguageMode) -> Result<Transcript, Error> {
        self.0.clone()
    }
}

/// Names an utterance by its length: "sentence 2" for the second capture.
struct CountingTranscriber;

impl Transcriber for CountingTranscriber {
    fn transcribe(&self, wav: &[u8], _language: LanguageMode) -> Result<Transcript, Error> {
        let n = (wav.len() + SAMPLES_PER_CAPTURE) / (2 * SAMPLES_PER_CAPTURE);
        Ok(Transcript::new(format!("sentence {n}"), Some("en")))
    }
}

struct EchoCleaner;

impl Cleaner for EchoCleaner {
    fn clean(&self, transcript: &str, _target: &TargetContext) -> Result<String, Error> {
        Ok(transcript.to_owned())
    }
}

struct FixedCleaner(&'static str);

impl Cleaner for FixedCleaner {
    fn clean(&self, _transcript: &str, _target: &TargetContext) -> Result<String, Error> {
        Ok(self.0.to_owned())
    }
}

const SAMPLES_PER_CAPTURE: usize = 1600;

/// `n` × 0.1 s of audio with a real signal in it, so the silence guard passes.
fn audio(n: usize) -> Vec<u8> {
    (0..SAMPLES_PER_CAPTURE * n.max(1))
        .flat_map(|i| (if i % 2 == 0 { 8000i16 } else { -8000 }).to_le_bytes())
        .collect()
}

fn session_with(
    transcript: Result<Transcript, Error>,
    configure: impl FnOnce(&mut DictateConfig),
) -> (Session<FakePlatform>, Rc<RefCell<World>>) {
    let mut config = DictateConfig { minimum_hold_ms: 0, ..DictateConfig::default() };
    configure(&mut config);

    let world = Rc::new(RefCell::new(World { focused: 1, has_device: true, ..World::default() }));
    let pipeline = DictationPipeline::new(
        Box::new(FixedTranscriber(transcript)),
        Box::new(FixedCleaner("So the thing is.")),
        config.clone(),
    );
    let session = Session::new(FakePlatform(Rc::clone(&world)), config, pipeline, Arc::new(DiagnosticLog::disabled()));
    (session, world)
}

fn session() -> (Session<FakePlatform>, Rc<RefCell<World>>) {
    session_with(Ok(Transcript::new("um so the thing is", Some("en"))), |_| {})
}

/// Pipelines that only run when the test says so, and name each utterance.
fn queued_session() -> (Session<FakePlatform>, Rc<RefCell<World>>) {
    let config = DictateConfig { minimum_hold_ms: 0, ..DictateConfig::default() };
    let world = Rc::new(RefCell::new(World { focused: 1, has_device: true, defer_jobs: true, ..World::default() }));
    let pipeline = DictationPipeline::new(Box::new(CountingTranscriber), Box::new(EchoCleaner), config.clone());
    let session = Session::new(FakePlatform(Rc::clone(&world)), config, pipeline, Arc::new(DiagnosticLog::disabled()));
    (session, world)
}

/// Runs the pipeline of the `index`-th utterance and hands its result over.
fn complete(session: &mut Session<FakePlatform>, world: &Rc<RefCell<World>>, index: usize) {
    let job = world.borrow_mut().jobs[index].take().expect("each pipeline runs once");
    job();
    session.finish();
}

fn dictate(session: &mut Session<FakePlatform>) {
    dictate_without_finishing(session);
    session.finish();
}

fn dictate_without_finishing(session: &mut Session<FakePlatform>) {
    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
}

#[test]
fn text_is_typed_into_the_window_focused_at_press_time() {
    let (mut session, world) = session();

    dictate(&mut session);

    let world = world.borrow();
    assert_eq!(world.typed, ["So the thing is. "]);
    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(world.states, [SessionState::Recording, SessionState::Transcribing, SessionState::Idle]);
}

#[test]
fn a_focus_change_sends_the_text_to_the_clipboard_and_types_nothing() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    world.borrow_mut().focused = 2;
    session.end(StopReason::Released);
    session.finish();

    let world = world.borrow();
    assert!(world.typed.is_empty(), "nothing may be typed into a window the user did not dictate into");
    assert_eq!(world.clipboard.last().map(String::as_str), Some("So the thing is."));
    assert_eq!(world.notices, ["Your text is on the clipboard"]);
}

#[test]
fn focus_that_leaves_and_returns_before_delivery_still_types() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    world.borrow_mut().focused = 2;
    session.end(StopReason::Released);
    world.borrow_mut().focused = 1;
    session.finish();

    assert_eq!(world.borrow().typed, ["So the thing is. "]);
}

#[test]
fn a_tap_is_discarded_without_a_word_to_the_user() {
    let (mut session, world) = session_with(Ok(Transcript::new("x", None)), |c| c.minimum_hold_ms = 60_000);

    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
    session.finish();

    let world = world.borrow();
    assert!(world.typed.is_empty());
    assert!(world.notices.is_empty());
    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(session.utterance_count(), 0);
}

#[test]
fn a_press_while_an_utterance_is_processed_starts_recording_at_once() {
    let (mut session, world) = queued_session();

    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
    session.begin(Instant::now(), Vec::new());

    assert_eq!(session.state(), SessionState::Recording);
    assert_eq!(world.borrow().captures_started, 2, "the press while processing must not be ignored");
}

#[test]
fn utterances_are_typed_in_press_order_when_the_later_one_finishes_first() {
    let (mut session, world) = queued_session();

    dictate_without_finishing(&mut session);
    dictate_without_finishing(&mut session);

    complete(&mut session, &world, 1);
    assert!(world.borrow().typed.is_empty(), "the second sentence may not overtake the first");

    complete(&mut session, &world, 0);

    let world = world.borrow();
    assert_eq!(world.typed, ["sentence 1 ", "sentence 2 "]);
    assert_eq!(
        world.states,
        [SessionState::Recording, SessionState::Transcribing, SessionState::Recording, SessionState::Transcribing, SessionState::Idle]
    );
    assert_eq!(session.state(), SessionState::Idle);
}

#[test]
fn each_utterance_goes_only_to_the_window_focused_at_its_own_press() {
    let (mut session, world) = queued_session();

    dictate_without_finishing(&mut session);
    world.borrow_mut().focused = 2;
    dictate_without_finishing(&mut session);

    complete(&mut session, &world, 0);
    complete(&mut session, &world, 1);

    let world = world.borrow();
    assert_eq!(world.typed, ["sentence 2 "], "the first sentence may not be typed into the second window");
    assert!(world.clipboard.iter().any(|text| text == "sentence 1"));
    assert_eq!(world.notices, ["Your text is on the clipboard"]);
}

#[test]
fn nothing_is_typed_while_the_hotkey_is_held() {
    let (mut session, world) = queued_session();

    dictate_without_finishing(&mut session);
    session.begin(Instant::now(), Vec::new());
    complete(&mut session, &world, 0);

    assert!(world.borrow().typed.is_empty(), "a keystroke while the hotkey is held combines with it");

    session.end(StopReason::Released);
    assert_eq!(world.borrow().typed, ["sentence 1 "]);

    complete(&mut session, &world, 1);
    assert_eq!(world.borrow().typed, ["sentence 1 ", "sentence 2 "]);
}

#[test]
fn a_tap_while_an_utterance_waits_delivers_it_and_records_nothing() {
    let (mut session, world) = queued_session();
    session.config.minimum_hold_ms = 60_000;

    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
    assert!(world.borrow().jobs.is_empty(), "a tap is not transcribed");

    session.config.minimum_hold_ms = 0;
    dictate_without_finishing(&mut session);
    session.config.minimum_hold_ms = 60_000;
    session.begin(Instant::now(), Vec::new());
    complete(&mut session, &world, 0);
    session.end(StopReason::Released);

    let world = world.borrow();
    assert_eq!(world.typed.len(), 1);
    assert_eq!(world.jobs.len(), 1);
    assert_eq!(session.state(), SessionState::Idle);
}

#[test]
fn a_release_without_a_press_does_nothing() {
    let (mut session, world) = session();

    session.end(StopReason::Reconciled);
    session.finish();

    assert!(world.borrow().states.is_empty());
}

#[test]
fn typing_that_fails_leaves_the_text_on_the_clipboard() {
    let (mut session, world) = session();
    world.borrow_mut().typing_fails = true;

    dictate(&mut session);

    let world = world.borrow();
    assert_eq!(world.clipboard.last().map(String::as_str), Some("So the thing is."));
    assert_eq!(world.notices, ["Your text is on the clipboard"]);
}

#[test]
fn a_failed_transcription_is_reported_and_nothing_is_typed() {
    let (mut session, world) = session_with(Err(Error::new("401")), |_| {});

    dictate(&mut session);

    let world = world.borrow();
    assert!(world.typed.is_empty());
    assert!(world.clipboard.is_empty());
    assert_eq!(world.notices, ["Transcription failed"]);
    assert!(session.recent.is_empty());
}

#[test]
fn without_a_microphone_the_press_says_so_and_starts_nothing() {
    let (mut session, world) = session();
    world.borrow_mut().has_device = false;

    session.begin(Instant::now(), Vec::new());

    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(world.borrow().notices, ["No microphone found"]);
    assert_eq!(world.borrow().captures_started, 0);
}

#[test]
fn a_device_that_fails_to_open_returns_to_idle_and_says_why() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    session.capture_failed();

    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(world.borrow().notices, ["The microphone would not open"]);
}

#[test]
fn delivered_text_is_kept_in_the_recent_list() {
    let (mut session, _world) = session();

    dictate(&mut session);

    assert_eq!(session.recent.items().first().map(|u| u.text.as_str()), Some("So the thing is."));
    assert_eq!(session.utterance_count(), 1);
}

#[test]
fn the_limit_tells_the_user_and_still_transcribes() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    std::thread::sleep(Duration::from_millis(1));
    session.end(StopReason::Limit);
    session.finish();

    let world = world.borrow();
    assert!(world.notices[0].starts_with("Recording stopped at"));
    assert_eq!(world.typed, ["So the thing is. "]);
}

#[test]
fn an_overloaded_service_tells_the_user_to_wait_and_types_nothing() {
    let busy = r#"{"detail":{"code":"system_busy","message":"We are sorry, the system is experiencing heavy traffic."}}"#;
    let (mut session, world) = session_with(Err(Error::from_response(Service::ElevenLabs, 429, busy)), |_| {});

    dictate(&mut session);

    let world = world.borrow();
    assert!(world.typed.is_empty());
    assert_eq!(world.notices, ["ElevenLabs is overloaded"]);
    assert!(world.details[0].contains("Wait a few minutes"));
    assert!(!world.details[0].contains("429"));
}

#[test]
fn a_device_error_is_logged_but_the_user_is_told_what_to_do() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    session.capture_failed();

    let world = world.borrow();
    assert!(!world.details[0].contains("device busy"));
    assert!(world.details[0].contains("Microphone"));
}
