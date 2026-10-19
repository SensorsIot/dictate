# dictate — working notes for Claude

Push-to-talk dictation for Windows. Rust, raw Win32 through `windows-sys`. See
[`docs/00-Overview.md`](docs/00-Overview.md) for the documentation map; this file
is only about collaborating on the repository.

## The one thing that will catch you out

**There is no Rust toolchain here** — not in the devcontainer, not on the
Windows machine. Deliberate — see `docs/Harness/00-Overview.md` §1. So:

- `cargo build` / `cargo test` / `cargo run` do not exist.
- Compile errors surface in GitHub Actions 3–5 minutes after a push.
- Every push to `main` uploads `dictate.exe` as the `dictate-exe` artifact;
  `gh run download <run> -n dictate-exe` fetches it for a desktop test.

Work accordingly: prefer API shapes you can verify from the Win32 documentation
over ones you half-remember, and batch compile-checkable changes into one push
rather than six. Where a `windows-sys` type is uncertain, `std::mem::zeroed()`
and `as _` compile whichever way it turns out.

## Layout

| Path | What |
|---|---|
| `dictate-core/` | Platform-free. Compiles **and is tested** on Linux. `#![forbid(unsafe_code)]`. |
| `dictate-windows/` | Every Win32 call. Builds `dictate.exe`. A stub `main` off Windows. |
| `docs/` | Three planes — WHAT / HOW / OPERATE. Start at `00-Overview.md`. Rationale sits beside what it explains: FSD §1.4, Harness §0. |
| `testing/test-plan.yaml` | Every test, its tier, and the requirements it discharges. |

**`dictate-core` must not reference a Windows-only API.** CI enforces it by
compiling and testing the workspace on `ubuntu-latest`. If you want Win32 in the
core, either the logic belongs in `dictate-windows`, or the Win32 part needs a
trait in the core.

## Before changing behaviour

Check `docs/Functionality/FSD.md` — the behaviour is probably already specified,
and these requirements exist because getting them wrong is expensive:

- **FR-6.4** — no newline is ever typed into a console. Enter in a shell runs the
  line. This is structural, not a prompt instruction, and the text sanitizer
  tests assert the *prohibited* outcome, not just the expected one.
- **FR-6.1/6.2** — the target window is pinned at key-press. If focus moved by
  delivery time, the text goes to the clipboard and *nothing* is typed.
- **FR-7.1** — cleanup failure degrades to the raw transcript. A scruffy sentence
  beats a lost one.
- **FR-11.12** — a cleanup result far longer than the transcript is the model
  talking to the user, and is never typed.

Changing any of these means updating the FSD in the same commit, not afterwards.

## The UI thread

It runs the keyboard hook. Windows skips a hook that does not answer within
~300 ms, and the hotkey's key-up then reaches the system (the Start menu
opens). So nothing on the UI thread may block: no device opens, no network, no
joins. Background threads post messages to the main window instead.

## Adding a test

Declare it in `testing/test-plan.yaml` with the requirement IDs it discharges,
then implement it. An undeclared test is invisible to traceability, so its
requirement still reads as unverified — correctly, because nobody can find the
evidence.

Implemented ≠ verified. Read `last_result`, not `status`.

## Conventions

- All `unsafe` lives in `dictate-windows`, beside the Win32 call it exists for.
- No `unwrap()` on anything that can fail at runtime in the Windows head.
- Keep the build at zero warnings; they are not promoted to errors, because with
  CI-only builds a promoted warning costs a round trip to discover.
- Comments state constraints the code cannot show (why `dwExtraInfo` is stamped,
  why the device opens off the UI thread). They don't narrate the next line.

## Don't

- Don't add a `.ico` — tray icons are drawn at runtime in `tray.rs`.
- Don't make the app elevated. An elevated hook cannot see input bound for
  unelevated windows, so dictation would stop working in ordinary apps.
- Don't write audio or transcripts to disk. FR-17.1 is a user decision, not an
  oversight, and the in-memory recent list is the agreed compromise.
- Don't special-case one PC's devices (the DAX inputs, the BRIO) in code. Device
  choice is configuration.
