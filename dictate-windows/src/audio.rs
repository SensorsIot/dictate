//! Records the default input device straight to the format Scribe wants:
//! 16 kHz, mono, 16-bit PCM.
//!
//! winmm (waveIn) rather than WASAPI: winmm converts to the requested format
//! for us, whereas WASAPI shared mode hands
//! back the device's mix format and would need a resampler in the hot path.
//!
//! Each open device belongs to one capture thread, which opens it, feeds it
//! buffers, and closes it. Nothing else ever touches the handle, and it is
//! closed only after waveInReset has returned every buffer. Stopping a device
//! from one thread while winmm still owns buffers handed out by another is a
//! race; when the close loses it, the handle is never released and the
//! microphone indicator stays lit until the process exits.
//!
//! **The UI thread never waits for a device.** It also runs the keyboard hook,
//! and Windows skips a hook that does not answer within LowLevelHooksTimeout.
//! Opening a device can take a second when the previous one is still closing —
//! a quick second press after a tap — and while the UI thread waited, the
//! hotkey's key-up went straight to the system: the Start menu opened, and the
//! session ended by reconciliation with no audio. So `start` only marks the
//! session; the device is opened on a background thread and kept only if the
//! session that asked for it is still running.
//!
//! Two modes, because opening a capture device costs real time:
//!   * default — open on press, close on release. The microphone indicator is
//!     only lit while recording.
//!   * KeepMicrophoneOpen — the device stays open and buffers are simply
//!     dropped outside a session.

use std::mem::size_of;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use dictate_core::wav;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Media::Audio::{
    waveInAddBuffer, waveInClose, waveInGetDevCapsW, waveInGetNumDevs, waveInOpen, waveInPrepareHeader, waveInReset, waveInStart,
    waveInUnprepareHeader, CALLBACK_EVENT, HWAVEIN, WAVEFORMATEX, WAVEHDR, WAVEINCAPSW, WAVE_MAPPER, WHDR_DONE,
};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

/// 50 ms buffers: small enough that release-to-upload is not waiting on a
/// half-full buffer, large enough to be cheap.
const BUFFER_BYTES: usize = 1600;
const BUFFER_COUNT: usize = 3;

/// The winmm device to record from, and a description for the log: the first
/// input whose name contains `wanted` (case-insensitive), otherwise the
/// Windows default. winmm names are cut at 31 characters, which still leaves
/// "Microphone (Logitech BRIO)" whole.
pub fn resolve_input(wanted: Option<&str>) -> (u32, String) {
    let Some(wanted) = wanted.map(str::trim).filter(|w| !w.is_empty()) else {
        return (WAVE_MAPPER, "default".into());
    };

    let inputs = list_inputs();
    match find_input(&inputs, wanted) {
        Some(index) => (index as u32, inputs[index].clone()),
        None => (WAVE_MAPPER, format!("default (no input matches '{wanted}')")),
    }
}

/// Every winmm input, indexed by its device id.
pub fn list_inputs() -> Vec<String> {
    let count = unsafe { waveInGetNumDevs() };
    (0..count)
        .map(|index| {
            let mut caps: WAVEINCAPSW = unsafe { std::mem::zeroed() };
            let found = unsafe { waveInGetDevCapsW(index as _, &mut caps, size_of::<WAVEINCAPSW>() as u32) } == 0;
            if !found {
                return format!("Input {index}");
            }

            let name_units = caps.szPname;
            let length = name_units.iter().position(|&u| u == 0).unwrap_or(name_units.len());
            String::from_utf16_lossy(&name_units[..length])
        })
        .collect()
}

/// The first input whose name contains `wanted`, case-insensitively.
pub fn find_input(inputs: &[String], wanted: &str) -> Option<usize> {
    let needle = wanted.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    inputs.iter().position(|name| name.to_lowercase().contains(&needle))
}

/// How long a device stays open after a session ends (FR-9.5). A press within
/// this window reuses it and captures at once; without it, a quick second
/// press waited for the previous device to finish closing — about three
/// seconds on a USB interface closed mid-startup — and lost its first second
/// of speech. Delivery never waits for this: the audio is handed over on
/// release, and the device closes in the background.
const LINGER: Duration = Duration::from_secs(3);

/// How long a press waits for the device to open before reporting failure.
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

pub enum RecorderEvent {
    /// MaximumRecordingSeconds was hit.
    LimitReached,
    /// How long after `start` the first buffer arrived. Separates a stalled
    /// capture stream from an interface that needs time to spin up — both
    /// otherwise read as "the microphone produced only silence".
    FirstBuffer(Duration),
    /// A device was closed; false if waveInClose reported failure, which is the
    /// signature of a handle Windows still considers in use.
    DeviceClosed(bool),
    /// The device for the current session could not be opened. The session has
    /// been ended; `take_open_error` says why.
    OpenFailed,
}

pub type Notify = Arc<dyn Fn(RecorderEvent) + Send + Sync>;

struct Capture {
    recording: bool,
    buffer: Vec<u8>,
    capped: bool,
    first_seen: bool,
    started: Instant,
    /// Incremented by every `start`, so a device that finishes opening after
    /// its session ended can tell it is no longer wanted.
    session: u64,
}

struct Inner {
    // Lock order: device before capture. The capture thread takes only capture.
    device: Mutex<Option<Device>>,
    capture: Mutex<Capture>,
    open_error: Mutex<Option<String>>,
    notify: Notify,
    keep_open: bool,
    input_device: Mutex<Option<String>>,
    max_bytes: usize,
}

pub struct Recorder {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Recorder {
    pub fn new(maximum_seconds: i32, keep_open: bool, input_device: Option<String>, notify: Notify) -> Self {
        let bytes_per_second = wav::SAMPLE_RATE as usize * wav::CHANNELS as usize * wav::BITS_PER_SAMPLE as usize / 8;
        Self {
            inner: Arc::new(Inner {
                device: Mutex::new(None),
                capture: Mutex::new(Capture {
                    recording: false,
                    buffer: Vec::new(),
                    capped: false,
                    first_seen: false,
                    started: Instant::now(),
                    session: 0,
                }),
                open_error: Mutex::new(None),
                notify,
                keep_open,
                input_device: Mutex::new(input_device),
                max_bytes: maximum_seconds.max(1) as usize * bytes_per_second,
            }),
        }
    }

    pub fn has_input_device() -> bool {
        unsafe { waveInGetNumDevs() > 0 }
    }

    /// Pays the one-time costs — loading winmm, driver initialisation, and in
    /// keep-open mode opening the device — at startup rather than on the first
    /// press. Off the UI thread: it opens a capture device.
    pub fn pre_warm(&self) {
        if !Self::has_input_device() {
            return;
        }

        let inner = Arc::clone(&self.inner);
        thread::spawn(move || {
            if inner.keep_open {
                Inner::open_for(&inner, None);
            } else if let Ok(device) = Device::open(Weak::new(), Arc::clone(&inner.notify), inner.device_id()) {
                // Pre-warming is an optimisation. If the device refuses now it
                // reports the real error on the first press, where there is a
                // user to tell.
                device.close(true);
            }
        });
    }

    /// Starts a session. Returns at once; the device opens in the background
    /// and audio counts from its first buffer.
    pub fn start(&self) {
        let session = {
            let mut capture = lock(&self.inner.capture);
            if capture.recording {
                return;
            }
            capture.session += 1;
            capture.buffer = Vec::with_capacity(self.inner.max_bytes.min(32_000 * 30));
            capture.capped = false;
            capture.first_seen = false;
            capture.started = Instant::now();
            capture.recording = true;
            capture.session
        };

        if lock(&self.inner.device).is_some() {
            return; // keep-open mode, already running
        }

        let inner = Arc::clone(&self.inner);
        thread::spawn(move || Inner::open_for(&inner, Some(session)));
    }

    /// Switches the microphone. Takes effect from the next session: a session
    /// in progress keeps the device it started on. An idle device left open
    /// by the linger (or by keep-open mode) is closed so the next press opens
    /// the new one.
    pub fn set_input_device(&self, input_device: Option<String>) {
        *lock(&self.inner.input_device) = input_device;

        if lock(&self.inner.capture).recording {
            return;
        }

        if let Some(device) = lock(&self.inner.device).take() {
            device.close(false);
        }

        if self.inner.keep_open {
            let inner = Arc::clone(&self.inner);
            thread::spawn(move || Inner::open_for(&inner, None));
        }
    }

    /// Why the last `OpenFailed` happened.
    pub fn take_open_error(&self) -> Option<String> {
        lock(&self.inner.open_error).take()
    }

    /// Stops recording and returns the captured PCM.
    pub fn stop(&self) -> Vec<u8> {
        let (pcm, session) = {
            let mut capture = lock(&self.inner.capture);
            if !capture.recording {
                return Vec::new();
            }
            capture.recording = false;
            (std::mem::take(&mut capture.buffer), capture.session)
        };

        if !self.inner.keep_open {
            // Closed after LINGER unless another session has started by then.
            // Detached, never awaited: the capture thread closes the device
            // and reports DeviceClosed.
            let inner = Arc::clone(&self.inner);
            thread::spawn(move || {
                thread::sleep(LINGER);

                let mut slot = lock(&inner.device);
                let idle = {
                    let capture = lock(&inner.capture);
                    !capture.recording && capture.session == session
                };

                if idle {
                    if let Some(device) = slot.take() {
                        drop(slot);
                        device.close(false);
                    }
                }
            });
        }

        pcm
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        {
            let mut capture = lock(&self.inner.capture);
            capture.recording = false;
            capture.buffer = Vec::new();
        }

        // Awaited here: this is shutdown, and a thread racing process exit is
        // how a handle leaks anyway.
        if let Some(device) = lock(&self.inner.device).take() {
            device.close(true);
        }
    }
}

impl Inner {
    /// Resolved on every open rather than once, so a camera plugged in after
    /// startup is picked up by the next press.
    fn device_id(&self) -> u32 {
        resolve_input(lock(&self.input_device).as_deref()).0
    }

    /// Opens a device on the calling (background) thread and keeps it only if
    /// it is still wanted: in keep-open mode always, otherwise only while the
    /// session that asked for it is recording. No lock is held while opening,
    /// so `stop` on the UI thread never waits for it.
    fn open_for(inner: &Arc<Inner>, session: Option<u64>) {
        match Device::open(Arc::downgrade(inner), Arc::clone(&inner.notify), inner.device_id()) {
            Ok(device) => {
                let mut slot = lock(&inner.device);
                let wanted = slot.is_none()
                    && (inner.keep_open || {
                        let capture = lock(&inner.capture);
                        capture.recording && Some(capture.session) == session
                    });

                if wanted {
                    *slot = Some(device);
                } else {
                    drop(slot);
                    device.close(false);
                }
            }
            Err(error) => {
                let mut capture = lock(&inner.capture);
                if session.is_some() && capture.recording && Some(capture.session) == session {
                    capture.recording = false;
                    capture.buffer = Vec::new();
                    drop(capture);

                    *lock(&inner.open_error) = Some(error);
                    (inner.notify)(RecorderEvent::OpenFailed);
                }
            }
        }
    }

    /// Called on the capture thread for every filled buffer.
    fn accept(&self, bytes: &[u8]) {
        let mut capture = lock(&self.capture);

        // In keep-open mode this runs continuously; buffers outside a session
        // are dropped here and never stored anywhere.
        if !capture.recording || capture.capped {
            return;
        }

        if !capture.first_seen {
            capture.first_seen = true;
            (self.notify)(RecorderEvent::FirstBuffer(capture.started.elapsed()));
        }

        let remaining = self.max_bytes.saturating_sub(capture.buffer.len());
        if remaining == 0 {
            capture.capped = true;
            (self.notify)(RecorderEvent::LimitReached);
            return;
        }

        let take = bytes.len().min(remaining);
        capture.buffer.extend_from_slice(&bytes[..take]);
    }
}

/// An auto-reset event shared by the capture thread and the handle that stops
/// it, closed when the last of the two lets go.
struct Event(isize);

impl Event {
    fn new() -> Result<Self, String> {
        let handle = unsafe { CreateEventW(null(), 0, 0, null()) };
        if handle.is_null() {
            Err("Could not create the capture event.".into())
        } else {
            Ok(Self(handle as isize))
        }
    }

    fn handle(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle()) };
    }
}

struct Device {
    stop: Arc<AtomicBool>,
    wake: Arc<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Device {
    fn open(sink: Weak<Inner>, notify: Notify, device_id: u32) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Event::new()?);
        let (ready_tx, ready_rx) = mpsc::channel();

        let thread = {
            let stop = Arc::clone(&stop);
            let wake = Arc::clone(&wake);
            thread::Builder::new()
                .name("dictate-capture".into())
                .spawn(move || capture_thread(sink, notify, stop, wake, ready_tx, device_id))
                .map_err(|e| format!("Could not start the capture thread: {e}"))?
        };

        let device = Self { stop, wake, thread: Some(thread) };

        match ready_rx.recv_timeout(OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(device),
            Ok(Err(message)) => {
                device.close(true);
                Err(message)
            }
            Err(_) => {
                device.close(false);
                Err(format!("The microphone did not open within {} s.", OPEN_TIMEOUT.as_secs()))
            }
        }
    }

    fn close(mut self, wait: bool) {
        self.stop.store(true, Ordering::SeqCst);
        unsafe { SetEvent(self.wake.handle()) };

        if wait {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn capture_thread(
    sink: Weak<Inner>,
    notify: Notify,
    stop: Arc<AtomicBool>,
    wake: Arc<Event>,
    ready: mpsc::Sender<Result<(), String>>,
    device_id: u32,
) {
    let header_size = size_of::<WAVEHDR>() as u32;
    let format = WAVEFORMATEX {
        wFormatTag: 1, // WAVE_FORMAT_PCM
        nChannels: wav::CHANNELS,
        nSamplesPerSec: wav::SAMPLE_RATE,
        nAvgBytesPerSec: wav::SAMPLE_RATE * u32::from(wav::CHANNELS) * u32::from(wav::BITS_PER_SAMPLE) / 8,
        nBlockAlign: wav::CHANNELS * wav::BITS_PER_SAMPLE / 8,
        wBitsPerSample: wav::BITS_PER_SAMPLE,
        cbSize: 0,
    };

    // The buffers must not move while winmm holds pointers into them, so they
    // are allocated once, up front, and outlive the device.
    let mut data: Vec<Vec<u8>> = (0..BUFFER_COUNT).map(|_| vec![0u8; BUFFER_BYTES]).collect();
    let mut headers: Vec<WAVEHDR> = Vec::with_capacity(BUFFER_COUNT);

    unsafe {
        let mut device: HWAVEIN = std::mem::zeroed();
        let opened = waveInOpen(&mut device, device_id, &format, wake.handle() as usize, 0, CALLBACK_EVENT);
        if opened != 0 {
            let _ = ready.send(Err(format!("Could not open the microphone (MMSYSERR {opened}).")));
            return;
        }

        for buffer in data.iter_mut() {
            let mut header: WAVEHDR = std::mem::zeroed();
            header.lpData = buffer.as_mut_ptr();
            header.dwBufferLength = BUFFER_BYTES as u32;
            headers.push(header);
        }

        for header in headers.iter_mut() {
            waveInPrepareHeader(device, header, header_size);
            waveInAddBuffer(device, header, header_size);
        }

        let started = waveInStart(device);
        if started != 0 {
            let _ = ready.send(Err(format!("Could not start the microphone (MMSYSERR {started}).")));
        } else {
            let _ = ready.send(Ok(()));

            while !stop.load(Ordering::SeqCst) {
                WaitForSingleObject(wake.handle(), 100);

                for (header, buffer) in headers.iter_mut().zip(data.iter()) {
                    if header.dwFlags & WHDR_DONE == 0 || stop.load(Ordering::SeqCst) {
                        continue;
                    }

                    if let Some(inner) = sink.upgrade() {
                        let recorded = (header.dwBytesRecorded as usize).min(buffer.len());
                        inner.accept(&buffer[..recorded]);
                    }

                    header.dwFlags &= !WHDR_DONE;
                    waveInAddBuffer(device, header, header_size);
                }
            }
        }

        // Reset returns every buffer to us; only then is unpreparing and
        // closing safe.
        waveInReset(device);
        for header in headers.iter_mut() {
            waveInUnprepareHeader(device, header, header_size);
        }
        let closed = waveInClose(device) == 0;

        drop(headers);
        drop(data);
        notify(RecorderEvent::DeviceClosed(closed));
    }
}
