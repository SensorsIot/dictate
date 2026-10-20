//! The session state machine of FSD §5, shared by every platform head.
//!
//! A head owns the event loop and the operating-system pieces — hotkey,
//! microphone, typing, the focused window, tray and overlay — and exposes them
//! through `Platform`. It calls `begin` on a press, `end` on a release, and
//! `finish` once the background job it was handed has run. Everything the FSD
//! says about a session in between happens here, the same on every platform.

use std::collections::{HashMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::diagnostic_log::{field, DiagnosticLog, Field};
use crate::{text_sanitizer, DictateConfig, DictationPipeline, RecentUtterances, TargetContext, Utterance, UtteranceStatus};

/// How many delivered utterances stay recoverable. In memory only (FR-17.2).
pub const RECENT_CAPACITY: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Recording,
    Transcribing,
}

impl SessionState {
    pub fn name(self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Recording => "recording",
            SessionState::Transcribing => "transcribing",
        }
    }
}

/// Why capture ended. Recorded on every `recording.stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The hotkey came up — the normal path.
    Released,
    /// No key-up arrived; the key was observed to be physically up.
    Reconciled,
    /// MaximumRecordingSeconds was hit.
    Limit,
}

impl StopReason {
    pub fn name(self) -> &'static str {
        match self {
            StopReason::Released => "Released",
            StopReason::Reconciled => "Reconciled",
            StopReason::Limit => "Limit",
        }
    }
}

/// What a platform head provides. Called only from the head's UI thread, and
/// none of it may block: on Windows that thread also answers the keyboard hook.
pub trait Platform {
    /// Identifies the window (or application) that has focus. Compared, never
    /// interpreted, by the session.
    type Target: Copy + PartialEq;

    fn focused(&self) -> Self::Target;

    /// Resolves a target to what the pipeline needs. May be slow; called after
    /// capture ends, while the user is waiting for the network anyway.
    fn describe(&self, target: Self::Target, extra_console_processes: &[String]) -> TargetContext;

    fn has_input_device(&self) -> bool;

    /// Returns at once; the device opens off the UI thread.
    fn start_capture(&mut self);

    /// The PCM captured since `start_capture`.
    fn stop_capture(&mut self) -> Vec<u8>;

    /// Why the device for the session in progress could not open.
    fn take_capture_error(&mut self) -> Option<String>;

    fn show_state(&mut self, state: SessionState);

    fn notify(&self, title: &str, detail: &str);

    fn type_text(&self, text: &str, config: &DictateConfig) -> Result<(), String>;

    fn copy_to_clipboard(&self, text: &str) -> Result<(), String>;

    /// Runs `job` off the UI thread, then arranges for `Session::finish` to be
    /// called on it.
    fn run_in_background(&self, job: Box<dyn FnOnce() + Send>);

    /// Working set in MB and open handles, logged with each utterance.
    fn process_counters(&self) -> (u64, u32) {
        (0, 0)
    }

    /// The paste chord named to the user, e.g. "Ctrl+V".
    fn paste_hint(&self) -> &'static str;
}

/// What the session remembers about an utterance while the pipeline runs.
struct Pending<T> {
    id: u64,
    pinned: Option<T>,
    held: Duration,
    pcm_bytes: usize,
    target: TargetContext,
}

pub struct Session<P: Platform> {
    pub platform: P,
    pub config: DictateConfig,
    pub log: Arc<DiagnosticLog>,
    pub recent: RecentUtterances,
    pipeline: Arc<DictationPipeline>,
    state: SessionState,
    recording: bool,
    pinned: Option<P::Target>,
    pressed_at: Instant,
    /// Utterances handed to the pipeline, in press order (FR-5.1).
    queue: VecDeque<Pending<P::Target>>,
    /// Pipeline results that arrived before an earlier utterance was delivered.
    done: HashMap<u64, Utterance>,
    next_id: u64,
    utterance_count: u64,
    results_tx: Sender<(u64, Utterance)>,
    results: Receiver<(u64, Utterance)>,
}

impl<P: Platform> Session<P> {
    pub fn new(platform: P, config: DictateConfig, pipeline: DictationPipeline, log: Arc<DiagnosticLog>) -> Self {
        let (results_tx, results) = mpsc::channel();
        Self {
            platform,
            config,
            log,
            recent: RecentUtterances::new(RECENT_CAPACITY),
            pipeline: Arc::new(pipeline),
            state: SessionState::Idle,
            recording: false,
            pinned: None,
            pressed_at: Instant::now(),
            queue: VecDeque::new(),
            done: HashMap::new(),
            next_id: 0,
            utterance_count: 0,
            results_tx,
            results,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn utterance_count(&self) -> u64 {
        self.utterance_count
    }

    /// Recording wins over transcribing: the user needs to see that the
    /// microphone is live, not that an earlier sentence is still on its way.
    fn show_state(&mut self) {
        let state = if self.recording {
            SessionState::Recording
        } else if !self.queue.is_empty() {
            SessionState::Transcribing
        } else {
            SessionState::Idle
        };
        if state != self.state {
            self.state = state;
            self.platform.show_state(state);
        }
    }

    /// A press. `origin` carries what the head knows about where the press came
    /// from (injected, lower integrity), logged with `recording.start`.
    pub fn begin(&mut self, pressed_at: Instant, origin: Vec<Field>) {
        // FR-5.1: one recording at a time; earlier utterances may still be in
        // the pipeline. Each keeps its own pinned target, so none can be
        // delivered into the target of another.
        if self.recording {
            return;
        }

        if !self.platform.has_input_device() {
            self.platform.notify("No microphone", "dictate found no audio input device.");
            return;
        }

        // FR-6.1: the target is recorded now and never re-read. Only the
        // handle — resolving it to a process name waits until capture ends.
        self.pinned = Some(self.platform.focused());
        self.pressed_at = pressed_at;

        self.platform.start_capture();

        let latency = self.pressed_at.elapsed();
        self.recording = true;
        self.show_state();

        let mut fields = vec![field("pressToStart", latency)];
        fields.extend(origin);
        self.log.event("recording.start", &fields);
    }

    /// The device for the session in progress could not open, and the
    /// recorder has already ended its side of it.
    pub fn capture_failed(&mut self) {
        let error = self.platform.take_capture_error().unwrap_or_else(|| "Unknown error.".into());
        self.log.event("mic.openFailed", &[field("error", error.as_str())]);

        if self.recording {
            self.recording = false;
            self.deliver_ready();
        }

        self.platform.notify("Could not start recording", &error);
    }

    pub fn end(&mut self, reason: StopReason) {
        if !self.recording {
            return;
        }

        self.recording = false;
        let held = self.pressed_at.elapsed();
        let pcm = self.platform.stop_capture();

        self.log.event(
            "recording.stop",
            &[field("reason", reason.name()), field("held", held), field("audioMB", megabytes(pcm.len()))],
        );

        // FR-5.2: an accidental tap costs nothing and says nothing — to the
        // user. It is still logged, or a session that ends here is invisible.
        if held.as_millis() < self.config.minimum_hold_ms.max(0) as u128 {
            self.log.event("recording.discarded", &[field("reason", "tap"), field("held", held)]);
            self.deliver_ready();
            return;
        }

        if reason == StopReason::Limit {
            self.platform.notify(
                "Recording limit reached",
                &format!(
                    "Stopped after {}s. Transcribing what was captured.",
                    self.config.maximum_recording_seconds
                ),
            );
        }

        let pinned = self.pinned.take();
        let target = match pinned {
            Some(pinned) => self.platform.describe(pinned, &self.config.extra_console_processes),
            None => TargetContext::unknown(),
        };

        let id = self.next_id;
        self.next_id += 1;
        self.queue.push_back(Pending { id, pinned, held, pcm_bytes: pcm.len(), target: target.clone() });

        let pipeline = Arc::clone(&self.pipeline);
        let results = self.results_tx.clone();

        self.platform.run_in_background(Box::new(move || {
            // The pipeline degrades internally; a panic means something outside
            // its contract broke. It costs one utterance, never the session.
            let utterance = panic::catch_unwind(AssertUnwindSafe(|| pipeline.process(&pcm, &target)))
                .unwrap_or_else(|_| Utterance::fail("The pipeline hit an internal error."));
            let _ = results.send((id, utterance));
        }));

        // An earlier utterance may have finished while this one was recorded.
        self.deliver_ready();
    }

    /// A background job from `end` has run.
    pub fn finish(&mut self) {
        while let Ok((id, utterance)) = self.results.try_recv() {
            self.done.insert(id, utterance);
        }
        self.deliver_ready();
    }

    /// FR-5.1: delivers finished utterances in press order, and none while the
    /// hotkey is held — a typed keystroke would combine with a modifier hotkey.
    fn deliver_ready(&mut self) {
        while !self.recording {
            let Some(id) = self.queue.front().map(|pending| pending.id) else { break };
            let Some(utterance) = self.done.remove(&id) else { break };
            let Some(pending) = self.queue.pop_front() else { break };

            self.utterance_count += 1;
            self.log_outcome(&utterance, &pending);
            self.deliver(&utterance, pending.pinned);
        }
        self.show_state();
    }

    /// Records the shape of an utterance and the process counters alongside it.
    /// Length rather than text: enough to correlate with a delivery problem,
    /// and FR-17.1 stays intact.
    fn log_outcome(&self, utterance: &Utterance, pending: &Pending<P::Target>) {
        if !self.log.is_enabled() {
            return;
        }

        let (working_set_mb, handles) = self.platform.process_counters();

        self.log.event(
            "utterance",
            &[
                field("n", self.utterance_count),
                field("held", pending.held),
                field("audioMB", megabytes(pending.pcm_bytes)),
                field("status", utterance.status),
                field("chars", utterance.text.chars().count()),
                field("lang", utterance.language_code.as_deref()),
                field("target", pending.target.process_name.as_str()),
                field("console", pending.target.is_console),
                field("transcribe", utterance.transcribe_time),
                field("cleanup", utterance.cleanup_time),
                field("error", utterance.error.as_deref()),
                field("wsMB", working_set_mb),
                field("handles", handles),
            ],
        );
    }

    fn deliver(&mut self, utterance: &Utterance, pinned: Option<P::Target>) {
        if utterance.status == UtteranceStatus::Failed {
            self.platform.notify("Dictation failed", utterance.error.as_deref().unwrap_or("Unknown error."));
            return;
        }

        if !utterance.has_text() {
            return;
        }

        self.recent.add(utterance.clone());

        if utterance.status == UtteranceStatus::CleanupFailed {
            // Delivered verbatim rather than lost (FR-7.1), but the user is told
            // so an unusually rough sentence is explained.
            self.platform
                .notify("Delivered without cleanup", utterance.error.as_deref().unwrap_or("Cleanup failed."));
        }

        // Before delivery, not after: if typing fails, or lands somewhere the
        // user did not intend, the text is already one paste away. This covers
        // the case the focus check cannot see — the wrong window was focused
        // the whole time.
        if self.config.always_copy_to_clipboard {
            if let Err(error) = self.platform.copy_to_clipboard(&utterance.text) {
                self.log.event("clipboard.failed", &[field("error", error)]);
            }
        }

        // FR-6.2: the target that had focus at press time, or nothing.
        if Some(self.platform.focused()) != pinned {
            self.log.event("delivery", &[field("method", "clipboard"), field("reason", "focus-changed")]);
            let detail = format!("The text is on the clipboard — press {}.", self.platform.paste_hint());
            self.to_clipboard(&utterance.text, "Focus changed", &detail);
            return;
        }

        // A trailing space so consecutive dictations do not run together. Not
        // added on the clipboard path, where the user sees what they paste.
        let typed = if self.config.append_space_after_insert {
            text_sanitizer::with_trailing_space(&utterance.text)
        } else {
            utterance.text.clone()
        };

        match self.platform.type_text(&typed, &self.config) {
            Ok(()) => {
                self.log.event("delivery", &[field("method", "typed"), field("chars", typed.chars().count())]);
            }
            Err(error) => {
                self.log.event("delivery.failed", &[field("error", error.as_str())]);
                self.to_clipboard(&utterance.text, "Could not type the text", &error);
            }
        }
    }

    fn to_clipboard(&self, text: &str, title: &str, detail: &str) {
        match self.platform.copy_to_clipboard(text) {
            Ok(()) => self.platform.notify(title, detail),
            Err(error) => {
                self.platform.notify("Text could not be delivered", &format!("{detail} Clipboard also failed: {error}"))
            }
        }
    }
}

fn megabytes(bytes: usize) -> f64 {
    (bytes as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0
}
