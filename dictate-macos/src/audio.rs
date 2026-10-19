//! Microphone capture through CoreAudio (cpal), converted to 16 kHz mono PCM16.
//!
//! One worker thread owns the input stream: it opens it, keeps it through the
//! linger, and closes it, so nothing that can block on the audio system runs on
//! the main thread. The stream delivers the device's own format; each buffer
//! goes through a `Downsampler` on CoreAudio's thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use dictate_core::resample::Downsampler;
use dictate_core::wav;

/// FR-9.5: how long the device stays open after a session, so a quick second
/// press reuses it instead of waiting for it to reopen.
const LINGER: Duration = Duration::from_secs(3);

pub enum RecorderEvent {
    LimitReached,
    /// How long after `start` the first buffer arrived.
    FirstBuffer(Duration),
    /// A device was opened; its name, for the log.
    DeviceOpened(String),
    DeviceClosed,
    /// The device for the current session could not be opened. The session has
    /// been ended; `take_open_error` says why.
    OpenFailed,
    /// The stream reported an error. Audio captured so far is kept (FR-9.4).
    StreamError(String),
}

pub type Notify = Arc<dyn Fn(RecorderEvent) + Send + Sync>;

enum Command {
    Start(u64),
    Stop(u64),
    DeviceChanged,
    PreWarm,
}

struct Capture {
    recording: bool,
    buffer: Vec<u8>,
    capped: bool,
    first_seen: bool,
    started: Instant,
    session: u64,
    downsampler: Option<Downsampler>,
}

struct Inner {
    capture: Mutex<Capture>,
    open_error: Mutex<Option<String>>,
    input_device: Mutex<Option<String>>,
    /// Set by the stream's error callback; the next session reopens.
    broken: AtomicBool,
    notify: Notify,
    keep_open: bool,
    max_bytes: usize,
}

pub struct Recorder {
    inner: Arc<Inner>,
    commands: Sender<Command>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Recorder {
    pub fn new(maximum_seconds: i32, keep_open: bool, input_device: Option<String>, notify: Notify) -> Self {
        let bytes_per_second = wav::SAMPLE_RATE as usize * wav::CHANNELS as usize * wav::BITS_PER_SAMPLE as usize / 8;
        let inner = Arc::new(Inner {
            capture: Mutex::new(Capture {
                recording: false,
                buffer: Vec::new(),
                capped: false,
                first_seen: false,
                started: Instant::now(),
                session: 0,
                downsampler: None,
            }),
            open_error: Mutex::new(None),
            input_device: Mutex::new(input_device),
            broken: AtomicBool::new(false),
            notify,
            keep_open,
            max_bytes: maximum_seconds.max(1) as usize * bytes_per_second,
        });

        let (commands, received) = mpsc::channel();
        let worker = Arc::clone(&inner);
        thread::Builder::new().name("audio".into()).spawn(move || worker_loop(&worker, &received)).ok();

        Self { inner, commands }
    }

    pub fn has_input_device() -> bool {
        cpal::default_host().default_input_device().is_some()
    }

    /// In keep-open mode, opens the device now rather than on the first press.
    pub fn pre_warm(&self) {
        let _ = self.commands.send(Command::PreWarm);
    }

    /// Starts a session. Returns at once; audio counts from the first buffer.
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
            capture.downsampler = None;
            capture.recording = true;
            capture.session
        };

        let _ = self.commands.send(Command::Start(session));
    }

    /// Takes effect from the next session; an idle open device is closed.
    pub fn set_input_device(&self, input_device: Option<String>) {
        *lock(&self.inner.input_device) = input_device;
        let _ = self.commands.send(Command::DeviceChanged);
    }

    pub fn take_open_error(&self) -> Option<String> {
        lock(&self.inner.open_error).take()
    }

    /// Stops recording and returns the captured PCM. The device closes after
    /// the linger, in the background.
    pub fn stop(&self) -> Vec<u8> {
        let (pcm, session) = {
            let mut capture = lock(&self.inner.capture);
            if !capture.recording {
                return Vec::new();
            }
            capture.recording = false;
            (std::mem::take(&mut capture.buffer), capture.session)
        };

        let _ = self.commands.send(Command::Stop(session));
        pcm
    }
}

/// Every input device's name, in the order CoreAudio lists them.
pub fn list_inputs() -> Vec<String> {
    match cpal::default_host().input_devices() {
        Ok(devices) => devices.filter_map(|device| device.name().ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// The index of the first input whose name contains `wanted`, ignoring case.
pub fn find_input(inputs: &[String], wanted: &str) -> Option<usize> {
    let wanted = wanted.to_lowercase();
    inputs.iter().position(|name| name.to_lowercase().contains(&wanted))
}

fn worker_loop(inner: &Arc<Inner>, commands: &Receiver<Command>) {
    let mut stream: Option<cpal::Stream> = None;
    // The session whose end started the linger, and when it runs out.
    let mut linger: Option<(u64, Instant)> = None;

    loop {
        let wait = linger.map(|(_, until)| until.saturating_duration_since(Instant::now())).unwrap_or(Duration::MAX);

        match commands.recv_timeout(wait) {
            Ok(Command::Start(session)) => {
                linger = None;
                if inner.broken.swap(false, Ordering::SeqCst) {
                    stream = None;
                }
                if stream.is_none() {
                    match open(inner) {
                        Ok(opened) => stream = Some(opened),
                        Err(error) => inner.open_failed(session, error),
                    }
                }
            }
            Ok(Command::Stop(session)) => {
                if !inner.keep_open && stream.is_some() {
                    linger = Some((session, Instant::now() + LINGER));
                }
            }
            Ok(Command::DeviceChanged) => {
                if !lock(&inner.capture).recording {
                    linger = None;
                    if stream.take().is_some() {
                        (inner.notify)(RecorderEvent::DeviceClosed);
                    }
                    if inner.keep_open {
                        stream = open(inner).ok();
                    }
                }
            }
            Ok(Command::PreWarm) => {
                if inner.keep_open && stream.is_none() {
                    stream = open(inner).ok();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let Some((session, _)) = linger.take() else { continue };
                let idle = {
                    let capture = lock(&inner.capture);
                    !capture.recording && capture.session == session
                };
                if idle && stream.take().is_some() {
                    (inner.notify)(RecorderEvent::DeviceClosed);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

impl Inner {
    fn open_failed(&self, session: u64, error: String) {
        {
            let mut capture = lock(&self.capture);
            if !(capture.recording && capture.session == session) {
                return;
            }
            capture.recording = false;
            capture.buffer = Vec::new();
        }
        *lock(&self.open_error) = Some(error);
        (self.notify)(RecorderEvent::OpenFailed);
    }

    /// Runs on CoreAudio's thread for every buffer.
    fn accept(&self, samples: &[f32], rate: u32, channels: u16) {
        let mut capture = lock(&self.capture);
        if !capture.recording || capture.capped {
            return;
        }

        if !capture.first_seen {
            capture.first_seen = true;
            (self.notify)(RecorderEvent::FirstBuffer(capture.started.elapsed()));
        }

        let Capture { buffer, downsampler, .. } = &mut *capture;
        downsampler.get_or_insert_with(|| Downsampler::new(rate, channels)).push(samples, buffer);

        if capture.buffer.len() >= self.max_bytes {
            let limit = self.max_bytes;
            capture.buffer.truncate(limit);
            capture.capped = true;
            (self.notify)(RecorderEvent::LimitReached);
        }
    }
}

/// FR-9.6: the first input whose name contains `InputDevice`, else the
/// system default. Resolved on every open, so a device plugged in after
/// startup is used from the next press.
fn open(inner: &Arc<Inner>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let wanted = lock(&inner.input_device).clone();

    let chosen = wanted.as_deref().and_then(|wanted| {
        let wanted = wanted.to_lowercase();
        host.input_devices()
            .ok()?
            .find(|device| device.name().map(|name| name.to_lowercase().contains(&wanted)).unwrap_or(false))
    });
    let device = chosen.or_else(|| host.default_input_device()).ok_or("No audio input device was found.")?;
    let name = device.name().unwrap_or_else(|_| "unknown".into());

    let supported = device.default_input_config().map_err(|error| format!("{name}: {error}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, inner),
        SampleFormat::I16 => build::<i16>(&device, &config, inner),
        SampleFormat::I32 => build::<i32>(&device, &config, inner),
        SampleFormat::U16 => build::<u16>(&device, &config, inner),
        other => Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|error| format!("{name}: {error}"))?;

    stream.play().map_err(|error| format!("{name}: {error}"))?;
    (inner.notify)(RecorderEvent::DeviceOpened(name));
    Ok(stream)
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, inner: &Arc<Inner>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let rate = config.sample_rate.0;
    let channels = config.channels;
    let sink = Arc::clone(inner);
    let errors = Arc::clone(inner);
    let mut scratch: Vec<f32> = Vec::new();

    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                scratch.clear();
                scratch.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                sink.accept(&scratch, rate, channels);
            },
            move |error| {
                errors.broken.store(true, Ordering::SeqCst);
                (errors.notify)(RecorderEvent::StreamError(error.to_string()));
            },
            None,
        )
        .map_err(|error| error.to_string())
}
