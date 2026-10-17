# Rust port (branch `rust-port`)

A port of dictate from C#/.NET to Rust, done core first. The C# code under
`src/` is still what ships; nothing here is wired into a release.

| Crate | Ports | State |
|---|---|---|
| `dictate-core` | `src/Dictate.Core` | Ported, with every host test from `tests/Dictate.Core.Tests` |
| `dictate-windows` | `src/Dictate.Windows` | Ported; builds in CI. **Not yet run on a desktop** |

The rule from `CLAUDE.md` carries over: **`dictate-core` must not touch a
Windows-only API.** CI compiles and tests it on `ubuntu-latest`.

## Where it deliberately differs from the C# core

- **Synchronous.** The pipeline runs on a worker thread, one utterance at a
  time, so `reqwest::blocking` replaces async/await and there is no runtime.
  Cancellation is by timeout only. The C# `CancellationToken` was used for
  shutdown, which the Windows head will handle by dropping the worker.
- **No Anthropic SDK.** Cleanup posts to the Messages API directly. The request
  body and response parsing are pure functions with their own tests, which
  also covers FR-11.8 (temperature 0) at the host tier for the first time.
- **Config enums** are read by name, case-insensitively, as before. Numeric
  enum values, which System.Text.Json also accepted, are not.

## The Windows head

Raw Win32 through `windows-sys`, on one UI thread with one message loop, the
same shape as the WinForms app. Module for module:

| C# | Rust |
|---|---|
| `HotkeyListener` | `hotkey.rs` — same reconciliation and symmetric suppression |
| `AudioRecorder` (NAudio WaveInEvent) | `audio.rs` — winmm `waveIn` directly |
| `TextInjector`, `WindowInspector`, `CredentialStore` | `injector.rs`, `window_inspector.rs`, `credentials.rs` |
| `Overlay`, `TrayIcons` + `NotifyIcon`, `AuthForm` | `overlay.rs`, `tray.rs`, `auth.rs` |
| `DictateApp`, `Program` | `app.rs`, `main.rs` |
| `app.manifest` | `build.rs` (embed-manifest: asInvoker, per-monitor-v2 DPI, Common Controls v6) |

Differences worth knowing before testing:

- **Capture threads own their device.** Each open microphone belongs to one
  thread that opens it, feeds it, resets it and closes it. Nothing else touches
  the handle, which removes the stop/dispose race the C# recorder had to work
  around.
- **The pipeline runs on a worker thread** and posts its result back, instead
  of `async void` on the UI thread.
- **Same credential targets** (`dictate:elevenlabs`, `dictate:anthropic`) and the
  same `config.json`, so the two builds share keys and settings. Quit one before
  starting the other — two hooks on one hotkey both fire.
- **Process counters in the log** carry working set and handle count; the
  thread count is gone.

CI uploads the release build as the `dictate-rust-exe` artifact on every push
to this branch.

## Running

```
cd rust
cargo test
```

There is no Rust toolchain on the development machine either; like the .NET
build, this compiles in CI (`rust-core` job in `.github/workflows/ci.yml`).
