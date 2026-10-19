use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dictate_core::{field, DiagnosticLog, RecentUtterances, Utterance, UtteranceStatus};

// ---- RecentUtterances -------------------------------------------------------

fn text(text: &str) -> Utterance {
    Utterance { status: UtteranceStatus::Ok, text: text.to_owned(), ..Utterance::fail("") }
}

fn texts(recent: &RecentUtterances) -> Vec<String> {
    recent.items().into_iter().map(|u| u.text).collect()
}

#[test]
fn newest_comes_first() {
    let mut recent = RecentUtterances::new(5);

    recent.add(text("one"));
    recent.add(text("two"));

    assert_eq!(texts(&recent), ["two", "one"]);
}

#[test]
fn the_oldest_is_dropped_once_capacity_is_exceeded() {
    let mut recent = RecentUtterances::new(3);

    for word in ["a", "b", "c", "d", "e"] {
        recent.add(text(word));
    }

    assert_eq!(recent.len(), 3);
    assert_eq!(texts(&recent), ["e", "d", "c"]);
}

#[test]
fn capacity_of_one_keeps_only_the_latest() {
    let mut recent = RecentUtterances::new(1);

    recent.add(text("first"));
    recent.add(text("second"));

    assert_eq!(texts(&recent), ["second"]);
}

#[test]
fn items_is_a_snapshot_that_adding_does_not_disturb() {
    // The tray menu enumerates this while rebuilding itself. A live view is
    // what crashed the C# version.
    let mut recent = RecentUtterances::new(5);
    recent.add(text("one"));

    let snapshot = recent.items();
    recent.add(text("two"));

    assert_eq!(snapshot.len(), 1);
}

#[test]
fn clear_empties_it() {
    let mut recent = RecentUtterances::new(5);
    recent.add(text("one"));

    recent.clear();

    assert!(recent.is_empty());
    assert!(recent.items().is_empty());
}

#[test]
#[should_panic(expected = "Capacity")]
fn a_capacity_below_one_is_rejected() {
    let _ = RecentUtterances::new(0);
}

// ---- DiagnosticLog ----------------------------------------------------------

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn build() -> (DiagnosticLog, Sink) {
    let sink = Sink::default();
    (DiagnosticLog::to(sink.clone()), sink)
}

#[test]
fn an_event_carries_its_name_and_fields() {
    let (log, sink) = build();

    log.event("utterance", &[field("n", 3), field("status", UtteranceStatus::Ok)]);

    let line = sink.contents();
    assert!(line.contains("utterance"));
    assert!(line.contains("n=3"));
    assert!(line.contains("status=Ok"));
}

#[test]
fn timespans_are_reported_in_milliseconds() {
    let (log, sink) = build();

    log.event("timing", &[field("transcribe", Duration::from_micros(1_234_700))]);

    assert!(sink.contents().contains("transcribe=1235ms"));
}

#[test]
fn newlines_in_a_value_cannot_forge_extra_log_lines() {
    // API errors arrive multi-line. Left alone they would break one event into
    // several, and a log that can be spoofed by its own inputs is worse than
    // no log.
    let (log, sink) = build();

    log.event("failed", &[field("error", "line one\nline two\r\nline three")]);

    assert_eq!(sink.contents().trim_end().lines().count(), 1);
}

#[test]
fn a_very_long_value_is_truncated() {
    let (log, sink) = build();

    log.event("failed", &[field("error", "x".repeat(5000))]);

    assert!(sink.contents().chars().count() < 1000);
    assert!(sink.contents().contains('…'));
}

#[test]
fn a_null_value_is_rendered_rather_than_dropped() {
    let (log, sink) = build();

    log.event("utterance", &[field("lang", None::<&str>)]);

    assert!(sink.contents().contains("lang=-"));
}

#[test]
fn the_disabled_log_writes_nothing_and_says_so() {
    let log = DiagnosticLog::disabled();

    assert!(!log.is_enabled());

    // Must not panic: every call site uses this instance by default.
    log.event("ignored", &[field("a", 1)]);
}

#[test]
fn each_event_is_one_line() {
    let (log, sink) = build();

    log.event("one", &[field("a", 1)]);
    log.event("two", &[field("b", 2)]);

    assert_eq!(sink.contents().trim_end().lines().count(), 2);
}
