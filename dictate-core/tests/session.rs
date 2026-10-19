use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dictate_core::{
    Cleaner, DiagnosticLog, DictateConfig, DictationPipeline, Error, LanguageMode, Platform, Session, SessionState,
    StopReason, TargetContext, Transcriber, Transcript,
};

/// Everything the fake platform was asked to do, and the one thing a test
/// steers: which window has focus.
#[derive(Default)]
struct World {
    focused: u32,
    has_device: bool,
    typing_fails: bool,
    captures_started: u32,
    typed: Vec<String>,
    clipboard: Vec<String>,
    notices: Vec<String>,
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

    fn stop_capture(&mut self) -> Vec<u8> {
        some_audio()
    }

    fn take_capture_error(&mut self) -> Option<String> {
        Some("device busy".into())
    }

    fn show_state(&mut self, state: SessionState) {
        self.0.borrow_mut().states.push(state);
    }

    fn notify(&self, title: &str, _detail: &str) {
        self.0.borrow_mut().notices.push(title.to_owned());
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

    /// Synchronous: by the time `end` returns, the result is waiting for `finish`.
    fn run_in_background(&self, job: Box<dyn FnOnce() + Send>) {
        job();
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

struct FixedCleaner(&'static str);

impl Cleaner for FixedCleaner {
    fn clean(&self, _transcript: &str, _target: &TargetContext) -> Result<String, Error> {
        Ok(self.0.to_owned())
    }
}

/// 0.1 s of audio with a real signal in it, so the silence guard passes.
fn some_audio() -> Vec<u8> {
    (0..1600).flat_map(|i| (if i % 2 == 0 { 8000i16 } else { -8000 }).to_le_bytes()).collect()
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

fn dictate(session: &mut Session<FakePlatform>) {
    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
    session.finish();
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
    assert_eq!(world.notices, ["Focus changed"]);
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
fn a_press_while_transcribing_is_ignored_not_queued() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    session.end(StopReason::Released);
    session.begin(Instant::now(), Vec::new());

    assert_eq!(session.state(), SessionState::Transcribing);
    assert_eq!(world.borrow().captures_started, 1);

    session.finish();
    assert_eq!(world.borrow().typed.len(), 1);
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
    assert_eq!(world.notices, ["Could not type the text"]);
}

#[test]
fn a_failed_transcription_is_reported_and_nothing_is_typed() {
    let (mut session, world) = session_with(Err(Error::new("401")), |_| {});

    dictate(&mut session);

    let world = world.borrow();
    assert!(world.typed.is_empty());
    assert!(world.clipboard.is_empty());
    assert_eq!(world.notices, ["Dictation failed"]);
    assert!(session.recent.is_empty());
}

#[test]
fn without_a_microphone_the_press_says_so_and_starts_nothing() {
    let (mut session, world) = session();
    world.borrow_mut().has_device = false;

    session.begin(Instant::now(), Vec::new());

    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(world.borrow().notices, ["No microphone"]);
    assert_eq!(world.borrow().captures_started, 0);
}

#[test]
fn a_device_that_fails_to_open_returns_to_idle_and_says_why() {
    let (mut session, world) = session();

    session.begin(Instant::now(), Vec::new());
    session.capture_failed();

    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(world.borrow().notices, ["Could not start recording"]);
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
    assert_eq!(world.notices, ["Recording limit reached"]);
    assert_eq!(world.typed, ["So the thing is. "]);
}
