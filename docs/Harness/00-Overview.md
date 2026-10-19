# Harness — how dictate is built and changed

The HOW plane. Nothing here is externally observable; if a statement could be
checked by a black-box tester, it belongs in the [FSD](../Functionality/dictate_FSD.md)
instead.

## 0. Why it is built this way

Implementation design and the alternatives it does not use. Product-level
design — what the system must do — is in the [FSD](../Functionality/dictate_FSD.md) §1.4.

| Uses | Not used | Why |
|---|---|---|
| Rust, raw Win32 through `windows-sys` | C# / .NET · Python · Electron | The application is Win32 interop plus two HTTP calls. Rust does both with no runtime: one ~4 MB exe, about 22 MB in memory, nothing to install, and no SDK whose surface can drift under a CI-only build. Python is weakest exactly here — `ctypes` structs, hook wrappers, packagers that trip antivirus. `windows-sys` rather than the `windows` crate: plain C signatures, checkable against the Win32 documentation without a compiler. |
| `dictate-core` + `dictate-windows` in one Cargo workspace | A single crate · two repositories | Keeps the whole testable half runnable on a Linux CI runner, without two release cycles for a one-user tool. |
| Builds only in GitHub Actions | A local toolchain | Deliberate, with the round-trip cost accepted. See §1 — it shapes how you work here more than anything else. |
| winmm (`waveIn`) for capture | WASAPI | winmm converts to 16 kHz mono for us. WASAPI shared mode returns the device's own mix format — 48 kHz stereo float — and would need a resampler in the hot path for no quality gain at this bitrate. |
| One capture thread owns each device, opened off the UI thread | Opening and closing on the UI thread | The UI thread also runs the keyboard hook. Opening a device can block for a second while a previous one closes, and Windows skips a hook that does not answer within LowLevelHooksTimeout — the key-up then reaches the system and the Start menu opens. |
| Messages posted to one window, state in a thread-local | Shared state behind locks | The hook, the capture threads and the pipeline worker never touch session state; they post to the main window and the work happens on the next pump. Anything that runs a modal loop — tray menu, key dialog, message box — is called with the state released, because it re-enters the window procedure. |
| No audio cues | Synthesised tones | A second audio device costs more than the tones are worth: an idle output endpoint takes seconds to spin up, and opening it stalls capture. The overlay says the same thing instantly. |
| An opt-in diagnostic log, metadata only | No log at all | Without it, diagnosing a shipped binary from another machine means reconstructing behaviour from Windows event logs, where a silent exit and a deliberate quit look identical. It records timings and counters, never dictated text, so FR-17.1 still holds. |
| Plain HTTP for both APIs | The Anthropic SDK | There is no official Rust SDK, and two small JSON requests cost less to own than a client surface to track. The request body and response parsing are pure functions with host tests. |
| Batch upload | Streaming transcription | Correctness first, then measure. `Transcriber` is the seam a streaming backend drops into without touching the pipeline. |

## 1. The constraint that shapes everything

**There is no Rust toolchain on any development machine.** This is deliberate (see §0) and it has consequences
you will feel within ten minutes:

- `cargo build`, `cargo test` and `cargo run` do not exist locally.
- A compile error is discovered by GitHub Actions, roughly 3–5 minutes after you
  push.
- No rust-analyzer: syntax highlighting, no type checking.

Two habits follow, and they are not optional here:

1. **Prefer API shapes you can verify from documentation over ones you
   half-remember.** A guessed signature costs a full CI round trip. Where a
   Win32 type is uncertain in `windows-sys` (a handle that is a pointer in one
   version and an integer in another), initialise it with `std::mem::zeroed()`
   and pass constants with `as _`, which compile either way.
2. **Push compile-checkable changes in batches.** One push that fixes six
   plausible errors beats six pushes fixing one each.

CI uploads the release build of every push to `main` as the `dictate-exe`
artifact, so a change can be tried on the desktop without a release.

## 2. Project layout

```
Cargo.toml          Workspace.
dictate-core/       Platform-free. Compiles and is tested on Linux.
dictate-windows/    Every Win32 call. Builds dictate.exe.
.cargo/config.toml  Static C runtime, so the exe needs nothing installed.
```

**The rule that keeps CI useful: `dictate-core` may not reference a
Windows-only API.** It is enforced by CI building and testing it on
`ubuntu-latest` (FR-20.3) — break the rule and the build goes red, which is the
point. If you find yourself wanting Win32 in the core, the logic wants to move
to `dictate-windows`, or the Win32 part wants a trait in the core.

`dictate-windows` compiles on Linux as a stub `main`, with every Win32 module
behind `#[cfg(windows)]`, so `cargo test --workspace` stays green there.
`dictate-macos` does the same behind `#[cfg(target_os = "macos")]`. Both heads
implement `dictate_core::Platform`; the session itself is in the core.

`dictate-macos` calls CoreGraphics, CoreFoundation and Accessibility through
C declarations in `ffi.rs`, checkable against Apple's headers like
`windows-sys` against Win32. Crates cover what has no small C surface: `cpal`
(CoreAudio), `tray-icon` (menu bar), `dispatch` (the main queue),
`security-framework` (Keychain), `mac-notification-sys`, `arboard`.

## 3. Seams, and why they exist

| Trait | In `dictate-core` | Exists so that |
|---|---|---|
| `Transcriber` | yes | A streaming Scribe backend can replace the batch one without touching the pipeline (§0) |
| `Cleaner` | yes | Cleanup can be swapped, benchmarked, or disabled entirely (`PassthroughCleaner`) |

Both are the reason `DictationPipeline` is testable without a network. Add a new
external dependency and it gets a trait in the core first.

## 4. Testing standard

### Tiers

| Tier | Where | When | What it can reach |
|---|---|---|---|
| `host` | `ubuntu-latest`, and `windows-latest` again | Every push | All of `dictate-core` |
| `ci` | `windows-latest` | Every push; release on tag | Compilation of `dictate-windows`, the exe |
| `desktop` | The user's Windows 11 machine | By hand | Hotkey, microphone, `SendInput`, focus, tray |

**The `desktop` tier is not automatable in this project's infrastructure**, and
the documentation says so rather than implying a CI job that does not exist. It
needs a real keyboard hook, a real microphone and a real foreground window.

When something fails on the desktop, the crash dialog says little. What finds a
crash: run the exe under a small debugger, catch the access
violation, and resolve the `user32.dll` frames against Microsoft's public
symbol server. The diagnostic log (`EnableDiagnosticLog`) answers most other
questions — `pressToStart`, `mic.firstBuffer`, `mic.close confirmed`.

### Writing a host test

- One behaviour per test; the name states the behaviour, not the function.
- Use the stubs in `tests/pipeline.rs` rather than a mocking crate — there are
  two traits and they have one method each.
- **Assert the prohibited outcome, not just the expected one.** `FR-6.4` is
  discharged by asserting the delivered text contains *no* newline; asserting it
  equals the happy-path string would pass while a newline slipped through in a
  case nobody thought of.

### Adding a test

Declare it in [`../../testing/test-plan.yaml`](../../testing/test-plan.yaml) with
the requirement IDs it discharges, then implement it. A test that exists but is
not declared is invisible to the traceability matrix, which means the requirement
still reads as unverified — correctly, because nobody can find its evidence.

### Order

Verification contract → plan declaration → executable test → code. **Per
requirement, not as project phases.** Running the steps as phases produces a plan
that reads finished while nothing is verified.

## 5. Conventions

- `#![forbid(unsafe_code)]` in `dictate-core`. All `unsafe` lives in
  `dictate-windows`, next to the Win32 call it exists for.
- No `unwrap()` on anything that can fail at runtime in the Windows head; a
  panic there costs the user's session. Window procedures and the hook catch
  panics at the boundary so none unwinds into user32.
- Warnings are not promoted to errors — with CI-only builds a promoted warning
  costs a round trip to discover — but the build is kept at zero warnings.
- Comments explain constraints the code cannot show — why `dwExtraInfo` is
  stamped, why the device opens off the UI thread, why cleanup has no
  `cache_control`. They do not narrate the next line.
- Match the surrounding style rather than importing a new one.

## 6. Release

Tag `v*` on `main`. The workflow stamps the tag's version into the workspace
`Cargo.toml` (every crate inherits it), runs the core tests, builds on
`windows-latest`, and attaches `dictate.exe` to a GitHub release; a second job
on `macos-latest` attaches `dictate-macos.zip`. Nothing is hand-edited.

`dictate-macos/bundle/build-app.sh` builds `dictate.app` for both
architectures, joins them with `lipo`, and signs and notarizes the bundle when
these GitHub Actions secrets are set: `MACOS_CERTIFICATE` (the Developer ID
Application certificate, `.p12`, base64), `MACOS_CERTIFICATE_PASSWORD`,
`APPLE_API_KEY` (App Store Connect API key, `.p8`, base64), `APPLE_API_KEY_ID`,
`APPLE_API_ISSUER_ID`. They exist nowhere else: not in the repository, not in a
log. Without them the bundle is signed ad hoc.

`Cargo.lock` is committed, as for any Rust binary, and CI builds with
`--locked`: a dependency changes only when someone changes it on purpose. With
no cargo on any development machine, a new dependency is resolved by the
`Lockfile` workflow, which runs on any branch push touching a `Cargo.toml` and
uploads the updated `Cargo.lock` as the `cargo-lock` artifact to commit.
