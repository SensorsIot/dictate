# dictate — Functional Specification

| | |
|---|---|
| **Status** | Released — v0.2.0. Requirements approved; host tier verified in CI, desktop tier partially discharged by measurement |
| **Version** | 0.2.0 |
| **Complexity** | Medium |
| **Plane** | WHAT — externally observable behaviour only. Build and code rules live in [`../Harness/`](../Harness/00-Overview.md); operating instructions in [`../UserDocumentation/Manual.md`](../UserDocumentation/Manual.md). |

Provenance tags: `[user]` a stated requirement · `[derived]` follows necessarily
from a stated requirement · `[assumed]` inferred, **not yet confirmed** ·
`[code]` read back from the implementation.

---

## 1. Overview

### 1.1 Purpose

dictate turns speech into typed text on a Windows PC or a Mac. The user holds a key,
speaks, and releases; a cleaned-up version of what they said is typed into
whichever window had focus when they started.

It exists because dictating prose into a text field is faster than typing it, and
because a verbatim speech-to-text transcript — fillers, false starts, no
punctuation, technical terms mangled — is not something anyone wants to send.

### 1.2 Users and context

One user, on a Windows 11 machine or a Mac, dictating into a mix of terminals, editors,
browsers and chat clients. Speech is German, English, or both in the same
sentence. The vocabulary includes project names and radio call signs that no
general speech model spells correctly.

### 1.3 What "done" looks like — the standard journey `[user]`

The FSD is a set of clauses; none of them describes an ordinary successful run.
This is that run, and it is the acceptance gate for workflow test W-01.

| # | Step | Observable |
|---|---|---|
| 1 | User launches `dictate.exe` | Tray icon appears, state *idle* |
| 2 | User places the caret in an editor | — |
| 3 | User presses and holds the hotkey | Overlay shows *recording* within 150 ms |
| 4 | User speaks one German sentence containing a vocabulary term | Overlay stays *recording* |
| 5 | User releases the hotkey | Overlay shows *transcribing* |
| 6 | Pipeline runs | Overlay clears within 2.5 s |
| 7 | — | The sentence appears at the caret: punctuated, no fillers, in German, vocabulary term spelled per the configured list |
| 8 | User checks the disk | No audio file, no transcript file, anywhere |

### 1.4 Design basis

Rationale lives with the requirement it explains, so that changing the
requirement forces you past the reason for it. This table covers only the
product-level design that shapes the whole system; implementation design —
language, project split, build strategy, audio stack — is in
[`../Harness/00-Overview.md`](../Harness/00-Overview.md), because it is HOW.

| Uses | Not used | Why |
|---|---|---|
| Windows 11 and macOS clients, one shared core | A Linux desktop · a service on the dev VM | The hotkey, the microphone and the target window all live on the desktop the user types at. A container on a headless VM has none of the three. Everything above the operating system is shared (§2.3), so both clients behave the same; Part F lists where macOS differs. |
| ElevenLabs Scribe, Infomaniak Whisper as the alternative | Local faster-whisper · Groq · Deepgram | A stated requirement (FR-10.1). The dev VM has 4 vCPUs and no GPU, so a local model there is not viable. Infomaniak keeps audio in Switzerland, at a latency cost (FR-10.7). |
| Claude Haiku 4.5 for cleanup, Infomaniak as the alternative | Gemini Flash · no cleanup at all | Same latency and cost class for a short cleanup pass. Infomaniak processes the text in Switzerland (FR-11.1). |
| Push-to-talk, hold | Toggle · toggle with silence auto-stop | Key release is an unambiguous end-of-utterance signal. No VAD, and no way to leave the microphone hot by accident. |
| Synthetic typing (`SendInput`) | Clipboard paste with restore · clipboard only | Works in terminals and over RDP without a per-application paste chord, and does not clobber the clipboard as a side effect of delivery. |
| German + English, auto with a pin | English only · full auto with no override | The user's work mixes both in one sentence, and auto-detection misfires on short utterances. |
| Hygiene + vocabulary + app context | Hygiene only · a full rewriting assistant | There are no voice commands: the model would have to separate content from instruction, and would eventually eat a wanted sentence. |
| Nothing persisted to disk | Transcript history · an audio ring buffer | No dictated content at rest, not even transcripts for tuning (FR-17.1). FR-17.2's in-memory buffer is what keeps "nothing is lost" honest. |
| Keys in Windows Credential Manager or the macOS login Keychain | Infisical CLI · a plaintext `.env` · a proxy service | Encrypted at rest under the user's account (FR-15.1, FR-22.9), works off the home LAN, no dependency on another machine being up. |
| Pin the target window, fall back to the clipboard | Best-effort typing into whatever has focus | Typing into whatever happens to be in front is how dictation lands a sentence in the wrong chat, or at a shell prompt. |
| Right Windows; Caps Lock with its tap kept, where there is no right Windows key | Right Ctrl · Right Alt · a mouse button | The hotkey is held for seconds, so it must mean nothing on its own. Right Ctrl is browser zoom; Right Alt is AltGr on a Swiss layout and binding it would cost `@ { } [ ] \`. Caps Lock is on the left of every laptop keyboard and keeps its own function on a quick tap (FR-8.9). |

Further product-level constraints are recorded at the requirement they shape:
FR-14.3 (no audio cues at all), FR-14.2 (a fixed
corner, not the cursor), FR-14.5 (no notification on success), FR-6.7 (always
copy to the clipboard), and in the Harness for the diagnostic log.

### 1.5 Scope boundary

The system under test ends at the last interface dictate owns: the `SendInput`
call and the clipboard write. Whether a given third-party application renders
those keystrokes correctly is outside the boundary — testing through a consumer
application destroys attribution and inherits its release schedule. The residual
risk is recorded as R-06.

---

## 2. Architecture

### 2.1 Context

```
   ┌──────────────┐   audio    ┌────────────────────┐
   │  microphone  │ ─────────► │                    │  HTTPS   ┌──────────────┐
   └──────────────┘            │      dictate       │ ───────► │  ElevenLabs  │
   ┌──────────────┐  key edge  │   (tray process)   │          │    Scribe    │
   │   keyboard   │ ─────────► │                    │          └──────────────┘
   └──────────────┘            │                    │  HTTPS   ┌──────────────┐
   ┌──────────────┐ keystrokes │                    │ ───────► │  Anthropic   │
   │ focused app  │ ◄───────── │                    │          │  Haiku 4.5   │
   └──────────────┘            └────────┬───────────┘          └──────────────┘
                                        │ read
                                        ▼
                            ┌────────────────────────┐
                            │  Credential Manager    │
                            │  %APPDATA%\dictate\    │
                            └────────────────────────┘
```

### 2.2 Data flow

```
hotkey down ──► pin target window ──► start capture (16 kHz mono PCM)
hotkey up   ──► stop capture ──► WAV ──► Scribe ──► raw transcript
                                                  └──► Haiku + target context ──► clean text
                                                                    └──► sanitise for target
                                                                              └──► focus unchanged? ──► SendInput
                                                                                    focus changed?  ──► clipboard + toast
```

### 2.3 Component layering `[derived]`

Strict one-way dependency: **L0 → L1 → L2**. The L0/L1 line is ownership — a
managed client to an external service is foundation; a hand-written driver or
decoder is an interface.

| Layer | Components | Project |
|---|---|---|
| **L2** Application logic | Dictation session state machine · delivery policy · degradation policy | `dictate-core` |
| **L1** Interfaces | `ScribeTranscriber` · `HaikuCleaner` · `cleanup_prompt` · `text_sanitizer` · `wav` · `audio::Recorder` · `hotkey` · `injector` · `window_inspector` | mixed |
| **L0** Foundation | `reqwest` · winmm · Credential Manager · configuration file | mixed |

**The layer split is not the project split**, and conflating them is the mistake
to avoid: `dictate-core` holds every layer's *platform-free* part, and
`dictate-windows` and `dictate-macos` every layer's operating-system part.
`dictate-core` shall not reference any platform-specific API (FR-20.3) — that is
what keeps L1 and L2 testable on a Linux runner, and what makes the two clients
behave the same.

### 2.4 Test architecture

| Tier | Runs where | Covers | Cost |
|---|---|---|---|
| **host** | `ubuntu-latest` in CI, every push | Everything in `dictate-core`: prompt construction, WAV framing, response parsing, sanitisation, pipeline degradation, configuration | Seconds |
| **desktop** | The user's Windows 11 machine, manually | Hotkey capture, microphone, `SendInput`, focus pinning, tray and overlay, Credential Manager | Minutes, human in the loop |
| **desktop-mac** | The user's Mac, manually | Event tap, CoreAudio capture, synthetic typing, focus pinning, menu bar, Keychain, permission prompts | Minutes, human in the loop |
| **ci** | `windows-latest` and `macos-latest` in CI, every push; release on tag | Both heads compile; a tag publishes the exe and the signed, notarized app | Minutes |

Nothing in the **desktop** tier can be automated in this project's
infrastructure: it needs a real keyboard hook, a real microphone and a real
foreground window. Those requirements are verified by a scripted manual
procedure, and the FSD says so rather than implying automation that does not
exist.

---

## 3. Phases

| Phase | Contents | Exit gate |
|---|---|---|
| **P1 Core** | `dictate-core` complete, host tier green | `cargo test` passes in CI |
| **P2 Client** | Hotkey, capture, injection, tray; end-to-end on the Windows machine | Journey (§1.3) passes manually |
| **P3 Hardening** | Focus-change fallback, degradation paths, limits | Deviation and negative tests pass |
| **P4 Release** | Tagged build publishing a self-contained exe | A downloaded exe runs on a clean Windows machine with nothing installed |

---

## 4. Risks

| ID | Risk | Consequence | Mitigation |
|---|---|---|---|
| R-01 | Text is typed into the wrong window after an alt-tab | A private sentence lands in the wrong chat, or a shell | FR-6.1 pins the target; FR-6.2 falls back to the clipboard |
| R-02 | A newline is typed into a terminal | Whatever is on the command line executes | FR-6.4 forbids it structurally, not by prompting |
| R-03 | Cleanup rewrites meaning or translates | The user sends something they did not say | FR-11.4 forbids translating and FR-11.5 forbids answering; cleanup never sees a request to answer |
| R-04 | Audio of everything spoken accumulates on disk | A laptop theft becomes a recording leak | FR-17.1: no audio or transcript is ever written |
| R-05 | An API key is readable on the machine | Account compromise | FR-15.1: DPAPI via Credential Manager, never a file |
| R-06 | A third-party app mishandles synthetic keystrokes | Dictation appears broken in that app | Accepted — outside the §1.5 boundary. Per-app chunking (FR-12.3) is the escape hatch |
| R-07 | CI-only builds mean compile errors surface late | Slow iteration | Accepted; see the Harness §0. Host tests keep the blast radius small |
| R-08 | A provider changes a model identifier | Every utterance fails | FR-10.2 and FR-11.2 make every model configurable; FR-7.4 surfaces the API's own error text |

### 4.1 Explicitly out of scope

- Voice commands ("delete that", "make it formal") — see §1.4.
- Streaming transcription — see the Harness §0.
- Any platform other than Windows 11 x64 and macOS.
- Multi-user or shared-machine operation.
- Persisting transcripts, audio, or usage history — see §1.4.

---

# Part A — Application logic (L2)

## 5. Dictation session

### 5.1 State model (normative)

The transition table is normative; any diagram is generated from it. Every
(state × event) pair below is handled or explicitly ignored.

| From | Event | Guard | To | Action |
|---|---|---|---|---|
| `Idle` | hotkey down | input device present | `Recording` | Pin target window; start capture; show overlay |
| `Idle` | hotkey down | no input device | `Idle` | Error toast |
| `Recording` | hotkey up | held ≥ `MinimumHoldMs` | `Transcribing` | Stop capture; overlay → *transcribing* |
| `Recording` | hotkey up | held < `MinimumHoldMs` | `Idle` | Discard audio; clear overlay; no sound |
| `Recording` | max duration reached | — | `Transcribing` | Stop capture; toast; proceed with what was captured |
| `Recording` | hotkey down | — | `Recording` | Ignored (auto-repeat) |
| `Recording` | hotkey observed physically up, no key-up received | — | as for *hotkey up* | Reconciled release (FR-8.7) |
| `Transcribing` | pipeline returns text | — | `Delivering` | — |
| `Transcribing` | pipeline returns failure | — | `Idle` | Error toast; clear overlay |
| `Transcribing` | hotkey down | — | `Transcribing` | Ignored — a new utterance may not start while one is in flight |
| `Delivering` | focus unchanged | — | `Idle` | Type text; clear overlay |
| `Delivering` | focus changed | — | `Idle` | Copy to clipboard; toast; clear overlay |
| `Delivering` | injection throws | — | `Idle` | Copy to clipboard; toast with the error |

**FR-5.1** [Must] `[derived]` The application shall hold exactly one dictation
session at a time; a hotkey press received in any state other than `Idle` shall
be ignored.

> **VC** — *Pre:* session in `Transcribing`. *Stimulus:* hotkey down. *Expect:* no
> new capture starts; state unchanged. *Must not:* two concurrent captures; audio
> from two utterances concatenated. *Tier:* desktop. *Evidence:* screen recording.

**FR-5.4** [Must] `[derived]` At most one instance shall run per user session.
A second launch shall say that dictate is already running and exit without
installing a hook.

> Two instances would both react to the hotkey: two recordings, the text typed
> twice. With FR-16.3 a leftover Startup-folder shortcut is enough to start a
> second. `--auth` is exempt; it installs nothing. *Tier:* desktop.

**FR-5.2** [Must] `[user]` A press held for less than `MinimumHoldMs`
(default 300 ms) shall be discarded without transcription and without sound.

> **VC** — *Pre:* `Idle`. *Stimulus:* tap the hotkey for < 300 ms. *Expect:* no
> network request; no overlay change that outlives the tap; state returns to `Idle`. *Must not:* an empty
> utterance reaching Scribe (it is billable). *Tier:* desktop.

**FR-5.3** [Must] `[user]` Capture shall stop automatically after
`MaximumRecordingSeconds` (default 120 s) and the captured audio shall still be
processed.

> **VC** — *Pre:* `Recording`. *Stimulus:* hold past the limit. *Expect:* capture
> stops within 1 s of the limit; a toast says so; pipeline runs on what was captured.
> *Must not:* unbounded memory growth; the utterance silently discarded.
> *Tier:* desktop.

## 6. Delivery policy

**FR-6.1** [Must] `[user]` The target window shall be recorded at hotkey-down and
shall not be re-read later in the session.

**FR-6.2** [Must] `[user]` If the foreground window at delivery time differs from
the pinned target, the text shall be placed on the clipboard and a notification
shown, and **no keystrokes shall be synthesised**.

> **VC** — *Pre:* utterance transcribed, target pinned to window A. *Stimulus:*
> focus window B before delivery. *Expect:* clipboard contains the text; toast
> names the fallback; window B receives nothing. *Must not:* a single character
> typed into B. *Tier:* desktop. *Evidence:* screen recording plus clipboard dump.

**FR-6.3** [Must] `[derived]` If injection fails for any reason, the text shall be
placed on the clipboard and the failure surfaced.

**FR-6.4** [Must] `[user]` When the target is a console window, the delivered text
shall contain no carriage return or line feed character.

> **VC** — *Pre:* target is a terminal. *Stimulus:* cleanup returns text
> containing `\n`. *Expect:* delivered text matches `^[^\r\n]*$`. *Must not:* any
> newline reaching `SendInput` — Enter in a shell runs the line. *Tier:* host
> (`TextSanitizerTests`). *Evidence:* CI test log.

**FR-6.5** [Must] `[derived]` A window shall be classified as a console if its
process name is in the built-in terminal list, or in the user's
`ExtraConsoleProcesses`, or its window class is `ConsoleWindowClass`.

**FR-6.6** [Should] `[user]` The delivered text of the most recent utterances
shall remain retrievable from the tray menu until the process exits.

**FR-6.7** [Should] `[user]` Every delivered utterance shall also be placed on
the clipboard, before injection is attempted.

> Covers the case FR-6.2 structurally cannot: the user had the wrong window
> focused for the whole utterance. Nothing changed, so the focus check passes
> and delivery looks successful — the text is simply in the wrong place. Copying
> before injection also means a failed or misdirected delivery is recoverable
> with Ctrl+V rather than through the tray menu.
>
> The cost is that dictating overwrites the clipboard, so it is configurable.

> **VC** — *Pre:* `AlwaysCopyToClipboard` true, target focused. *Stimulus:* one
> utterance. *Expect:* the text is typed **and** the clipboard holds it.
> *Must not:* a clipboard failure preventing or interrupting the typing.
> *Tier:* desktop.

## 7. Degradation

Ordered by preference. The principle: a scruffy sentence beats a lost one, and a
lost one beats a wrong one.

**FR-7.1** [Must] `[user]` If cleanup fails or returns empty, the raw transcript
shall be delivered instead, and the session marked as degraded.

> **VC** — *Pre:* Scribe succeeds, cleanup throws. *Stimulus:* run the pipeline.
> *Expect:* `Status = CleanupFailed`; `Text` equals the raw transcript; `Error`
> populated. *Must not:* the utterance dropped; an exception reaching the UI
> thread. *Tier:* host (`PipelineTests`). *Evidence:* CI test log.

**FR-7.2** [Must] `[derived]` If transcription fails, no text shall be delivered
and the failure shall be surfaced to the user.

**FR-7.3** [Must] `[derived]` If transcription succeeds but recognises nothing,
the session shall end without delivering text and without an error sound.

**FR-7.4** [Must] `[derived]` An error surfaced to the user shall include the
message returned by the failing service, truncated to 500 characters.

> Rationale: an invalid `model_id` comes back from Scribe as a 422 listing the
> values it accepts. Swallowing that text turns a 30-second fix into a debugging
> session.

---

# Part B — Interfaces (L1)

## 8. Hotkey

**FR-8.1** [Must] `[user]` The application shall detect press and release of a
configurable virtual-key code, default `0x5C` (Right Windows), globally —
regardless of which application has focus.

> The hotkey must be a key with no meaning of its own, because it is held for
> seconds at a time. Right Windows qualifies; Right Ctrl does not, since
> Ctrl+scroll is browser zoom and every zoom would hold the key past
> `MinimumHoldMs` and start a dictation. Right Alt is AltGr on a Swiss layout.
>
> Left and right are distinct virtual keys here, unlike Ctrl, which also has a
> generic code. `0x5B` is the left Windows key and carries most of the system
> shortcuts; only `0x5C` is free.

**FR-8.2** [Must] `[derived]` Keyboard auto-repeat shall not generate additional
press events.

**FR-8.3** [Must] `[derived]` Keystrokes synthesised by dictate itself shall not
be interpreted as hotkey events.

> Without this, dictating a sentence while the hotkey character is in the text
> re-triggers the session. Implemented by stamping `dwExtraInfo`.

**FR-8.4** [Should] `[user]` The hotkey shall be suppressed by default, and a
suppressed key-up shall be swallowed only if the matching key-down was.

> The default hotkey requires it: a Windows key released with nothing pressed in
> between opens the Start menu, so a passed-through hotkey opens it on every
> dictation. A user who moves the hotkey to an already-inert key should turn
> suppression off with it.
>
> The second clause is the cost of the first. Suppression that is not symmetric
> corrupts the system's key state: if the hook is installed while the key is
> already held — which happens whenever the application restarts mid-press — the
> key-down has already reached the system, and swallowing the key-up leaves
> Windows believing the key is still held. With a modifier that turns every
> subsequent keystroke into a chord, and the keyboard appears to fail.
>
> **VC** — *Pre:* hotkey physically held, dictate not running. *Stimulus:* start
> dictate, then release the key. *Expect:* `GetAsyncKeyState` reports the key up.
> *Must not:* a modifier left held at the system level. *Tier:* desktop.

**NFR-8.5** [Must] `[derived]` The application shall run unelevated.

> A low-level hook installed by an elevated process cannot see input destined for
> unelevated windows. Running as administrator would stop dictation working in
> ordinary applications.

**FR-8.6** [Must] `[assumed]` Every hotkey press shall be recorded with whether it
was synthesised by another process, and presses so synthesised shall be
ignorable by configuration (`IgnoreInjectedHotkey`, default off).

> `[assumed]` on the default only. dictate can rule out its own synthetic
> keystrokes by signature (FR-8.3) but not anyone else's, and for a push-to-talk
> an unasked-for press means an open microphone. Rejecting synthetic input cannot
> be the default, because remote-desktop tools deliver the user's genuine
> keystrokes the same way — so the origin is always recorded and rejecting it
> is a setting.

> dictate can rule out its own synthetic keystrokes by signature (FR-8.3) but not
> anyone else's. Mouse software, CAD input devices and macro tools all map buttons
> to bare modifiers, and for a push-to-talk an unasked-for press means an open
> microphone. Rejecting synthetic input cannot be the default, because
> remote-desktop tools deliver the user's genuine keystrokes the same way —
> so the origin is always recorded and the choice is left to the user.
>
> **VC** — *Pre:* `Idle`, `IgnoreInjectedHotkey` off. *Stimulus:* another process
> sends the hotkey via SendInput. *Expect:* the session starts and
> `recording.start` records `injected=True`. *Then:* with the setting on, no
> session starts and `hotkey.rejected` is logged. *Tier:* desktop.

**FR-8.7** [Must] `[derived]` A press whose release is never delivered shall be
released within the system's initial auto-repeat delay plus 250 ms, and a press
that is still held shall never be treated as released.

> A low-level hook does not receive input destined for a higher-integrity window.
> Hold the hotkey, let an elevated window take focus, release it there, and no
> key-up ever arrives — capture would otherwise run until the process exits.
> FR-5.3 bounds the damage; this removes it.
>
> The second half of the requirement is not padding. The system key state cannot
> carry this alone: a suppressed hotkey (FR-8.4) never reaches the system, so it
> reads as up for the whole time it is held. Auto-repeat is what survives
> suppression — the hook keeps receiving down-events either way — so a release
> requires both the key state and silence from the keyboard.
>
> The bound is read from the machine (`SPI_GETKEYBOARDDELAY`,
> `SPI_GETKEYBOARDSPEED`) rather than fixed at the slowest Windows permits, and
> two graces are kept: the full initial delay until the first repeat arrives, and
> four repeat periods after it. On a machine set to a 500 ms delay and 30 repeats
> per second that is 650 ms falling to 282 ms. Both are logged at startup as
> `graceInitial` and `graceRepeat`.
>
> **VC** — *Pre:* `Recording`. *Stimulus:* suppress the key-up (release while an
> elevated window has focus). *Expect:* the session ends within the initial delay
> plus 250 ms and `recording.stop` records `reason=Reconciled`. *Must not:* the
> capture device left open. *Tier:* desktop.
>
> **VC** — *Pre:* `Idle`, `SuppressHotkey` on. *Stimulus:* hold the hotkey for 3 s
> and speak. *Expect:* exactly one session, `held≈3000ms`, `reason=Released`.
> *Must not:* the hold fragmenting into repeated sessions discarded as taps.
> *Tier:* desktop.

**FR-8.8** [Should] `[user]` When the hotkey is a Windows key and the Start menu
takes the foreground directly from a window running as administrator, dictate
shall close it and say why it cannot hear the hotkey there.

> The hook never sees a press in an elevated window, so the key is not
> suppressed and its release opens Start. The key itself is invisible to
> dictate there (the async key state reads up throughout); only the Start menu
> arriving is visible. That cannot tell the two Windows keys apart, so the other
> one, pressed on purpose in an elevated window, is closed as well rather than
> leaving Start open beside the message. A click on the Start button is exempt: the cursor is
> on the taskbar. *Must not:* closing a Start menu opened from an ordinary
> window. *Tier:* desktop.

**FR-8.9** [Should] `[user]` With `ShortPressPassesThrough` on (default off)
and the hotkey suppressed, a press released before `MinimumHoldMs` shall be
replayed to the system as one tap of the same key, stamped as dictate's own
input (FR-8.3). A held press dictates as before. A release synthesised by
FR-8.7 is not replayed. Windows only.

> So a hotkey on a key with a function of its own keeps it on a tap: Caps Lock
> still toggles, and a long press dictates. The tap's effect arrives at release
> rather than at press. *Must not:* a replay when the key was not suppressed —
> it already reached the system, and Caps Lock would toggle back. *Tier:* host
> (H-24) for the decision; desktop for the replay.

## 9. Audio capture

**FR-9.1** [Must] `[derived]` Captured audio shall be 16 kHz, mono, 16-bit PCM.

**FR-9.2** [Must] `[derived]` Capture shall begin within 150 ms of hotkey-down.

> **VC** — *Pre:* `Idle`. *Stimulus:* hotkey down while speaking immediately.
> *Expect:* the first syllable appears in the transcript. *Must not:* a clipped
> first word. *Tier:* desktop.

**FR-9.3** [Must] `[derived]` If no input device is present, the session shall not
start and the user shall be told.

**FR-9.4** [Must] `[derived]` If the input device fails mid-utterance, audio
captured before the failure shall still be processed.

**FR-9.6** [Should] `[user]` When `InputDevice` is set, audio shall be captured
from the first input whose name contains it, case-insensitively; with no
match, from the Windows default input. The device in use shall be logged.

> The tray menu's **Microphone** submenu lists "Windows default" and every
> input, checks the one in use, and on selection switches from the next press
> and writes only `InputDevice` into config.json, leaving the user's other
> keys untouched.
>
> Lets dictation use one microphone (a webcam's, say) without making
> it the default for every other application. Resolved on every open, so a
> device plugged in after startup is used from the next press.

**FR-9.5** [Must] `[user]` When `KeepMicrophoneOpen` is off, the capture device
shall stay open for 3 s after a session ends and then close, unless a new
session has started in that time, which reuses it. Delivery of the ended
session's text shall not wait for the close.

> Without the linger, a press shortly after a tap waits for the previous device
> to finish closing (about 3 s for a USB interface closed mid-startup) and loses
> its first second of speech. The cost is the Windows microphone
> indicator staying lit 3 s longer after each dictation. *Must not:* text
> delivered later than without the linger; the device still open 5 s after the
> last session. *Tier:* desktop.

## 10. Transcription

**FR-10.1** [Must] `[user]` Audio shall be transcribed by the provider
`TranscriptionProvider` selects: `ElevenLabs` (default), Scribe via
`POST https://api.elevenlabs.io/v1/speech-to-text` authenticated with the
`xi-api-key` header; or `Infomaniak`, Whisper through Infomaniak AI Services
(FR-10.7). FR-10.3 to FR-10.6 describe Scribe.

**FR-10.2** [Must] `[user]` The transcription model shall be configurable as
`TranscriptionModel`. Unset, it is `ScribeModelId` (default `scribe_v2`) for
ElevenLabs and `whisper` for Infomaniak.

> It stays configurable so a future Scribe generation is a settings change,
> not a rebuild.

**FR-10.3** [Must] `[user]` When `Language` is `Auto` no `language_code` shall be
sent; when it is `German` or `English`, `de` or `en` shall be sent respectively.

> There is deliberately no second hotkey to pin the language for a single
> utterance: the config setting is enough. Note that a language misdetection is
> usually a symptom of truncated capture rather than of detection itself — a
> per-utterance override would mask that instead of surfacing it.

**FR-10.4** [Must] `[derived]` `tag_audio_events` and `diarize` shall both be
sent as `false`.

> `(laughter)` and speaker labels are noise when the output goes straight into a
> text field.

**FR-10.5** [Must] `[derived]` A non-2xx response shall raise an error carrying
the response body.

**FR-10.6** [Must] `[derived]` The request shall be abandoned after
`ScribeTimeoutSeconds` (default 15 s). For Infomaniak the bound covers the
upload and every result poll together.

**FR-10.7** [Must] `[user]` With `Infomaniak`, the audio shall be submitted to
`POST https://api.infomaniak.com/1/ai/{product}/openai/audio/transcriptions`
with a Bearer token, and `GET /1/ai/{product}/results/{batch}` polled until the
batch succeeds or fails; a failed batch is an error, never an empty transcript.
`{product}` is `InfomaniakProductId`, or the token's first AI Services product,
looked up on first use. A pinned `Language` is sent as `language`, and
`Vocabulary` as Whisper's `prompt`. The response format is `verbose_json`,
the only one that reports the detected language (FR-10.8).

> The API is asynchronous and each poll takes about a second on Infomaniak's
> side, so an utterance typically takes 3–4 s to transcribe (measured
> 2026-10-20: 2.7–7.7 s), against under 1 s for Scribe. Their API reference says
> a supplied language "will translate the output"; measured, German audio sent
> with `en` came back in German, so pinning is safe under FR-11.4. *Tier:* host
> (H-21) for the requests and parsing; desktop for latency.

**FR-10.8** [Won't] `[user]` dictate does not restrict automatic detection to
German and English. With `Language` `Auto` the provider chooses among all the
languages it knows, and a misdetection is delivered as transcribed.

> Neither Scribe nor Whisper accepts a set of allowed languages, so a
> restriction would mean redoing a third-language result as German and as
> English and keeping the surer one. Measured on 2026-10-20, forcing the wrong
> one of the two does not translate on either provider, so the redo would
> rarely change the text, while with Infomaniak it costs another 3–4 s.
> Declined for that trade. The detected language is logged (`lang`) for both
> providers, so a misdetection can be seen; pinning `Language` is the remedy.

> **VC** for FR-10.1/10.3/10.5 — *Pre:* a recorded Scribe response fixture.
> *Stimulus:* parse it. *Expect:* `text` and `language_code` extracted, whitespace
> trimmed; a response without `text` raises `TranscriptionException`. *Tier:* host
> (`ScribeParseTests`).

## 11. Cleanup

**FR-11.1** [Must] `[user]` The raw transcript shall be cleaned before delivery
by the provider `CleanupProvider` selects: `Anthropic` (default), Claude through
the Anthropic Messages API; or `Infomaniak`, through the OpenAI-compatible
`POST https://api.infomaniak.com/2/ai/{product}/openai/v1/chat/completions`,
with `stream` false and `reasoning_effort` `none`.

**FR-11.2** [Must] `[derived]` The cleanup model identifier shall be configurable
as `CleanupModel`. Unset, it is the provider's default: `claude-haiku-4-5` for
Anthropic, `google/gemma-4-31B-it` for Infomaniak.

> Unset rather than one fixed default, so switching provider never sends a
> Claude model name to Infomaniak. Gemma because, tested on 2026-10-20,
> Infomaniak's smaller `Ministral-3-14B` translated the English half of a mixed
> sentence and `Mistral-Small-4` answered a question it was given to clean —
> FR-11.4 and FR-11.5 failures. Infomaniak streams and reasons by default; both
> would only add latency.

**FR-11.13** [Must] `[user]` Each provider's key shall be stored separately, and
one Infomaniak key serves both transcription and cleanup. Switching provider
shall neither need nor delete another provider's key. The key prompt asks for
the transcription provider's key (required) and, if cleanup uses a different
provider, its key (optional).

**FR-11.3** [Must] `[user]` Cleanup shall remove fillers and false starts, and add
sentence punctuation and capitalisation.

**FR-11.4** [Must] `[user]` Cleanup shall not translate; the reply shall be in the
language spoken, including where two languages are mixed in one sentence.

> **VC** — *Pre:* the standard system prompt. *Stimulus:* inspect it. *Expect:* an
> explicit prohibition on translating. *Must not:* the prohibition absent — a
> German utterance silently returned in English is indistinguishable from good
> output until the user reads it. *Tier:* host (`CleanupPromptTests`) for the
> prompt; desktop for the behaviour.

**FR-11.5** [Must] `[user]` Cleanup shall not answer, explain, summarise or
continue the transcript.

**FR-11.12** [Must] `[derived]` A cleanup result longer than twice the raw
transcript plus 20 characters shall be treated as a cleanup failure under
FR-7.1: the raw transcript is delivered and the session marked degraded.

> FR-11.5 enforced structurally, because a prompt cannot guarantee it: given a
> very short fragment, the model can reply to the user instead ("I have no
> transcript to clean. Please provide the speech-to-text output you'd like me to
> process."), and that reply would otherwise be typed. Cleanup only removes and
> punctuates, so real output never approaches the bound. *Must not:* a reply to
> the user delivered as dictated text. *Tier:* host (`PipelineTests`).

**FR-11.6** [Must] `[user]` Terms listed in `Vocabulary` shall be spelled as
configured.

**FR-11.7** [Should] `[user]` The process name and window title of the pinned
target shall be supplied to cleanup so it can adapt formatting.

**FR-11.8** [Must] `[derived]` Cleanup shall run at `temperature = 0`.

> The same utterance must not clean up two different ways.

**FR-11.9** [Must] `[derived]` The request shall be abandoned after
`CleanupTimeoutSeconds` (default 10 s), degrading per FR-7.1.

**FR-11.10** [Should] `[derived]` If the cleanup provider has no key
configured, dictation shall still work, delivering the verbatim transcript.

**FR-11.11** [Must] `[user]` When `SwissSpelling` is on (default), the delivered
text shall contain no `ß` or `ẞ`; each shall be written `ss` / `SS`. This applies
to the raw-transcript fallback of FR-7.1 as well as to cleaned text.

> Swiss Standard German, not German German. Enforced as a deterministic
> replacement after cleanup, not as a prompt instruction, so a degraded delivery
> obeys it too. *Tier:* host (`TextSanitizerTests`, `PipelineTests`).

## 12. Text injection

**FR-12.1** [Must] `[user]` Text shall be delivered with `SendInput` using
Unicode scan codes (`KEYEVENTF_UNICODE`), not clipboard paste.

**FR-12.2** [Must] `[derived]` Characters outside the Basic Multilingual Plane
shall be delivered correctly as surrogate pairs.

**FR-12.3** [Should] `[derived]` Events shall be sent in batches of
`InjectionChunkSize` (default 200) with `InjectionChunkDelayMs` between batches.

> One `SendInput` call per paragraph rather than per character; the chunk size is
> the escape hatch for applications that drop fast input (R-06). No application
> in use has needed it, so the delay stays at zero — a `delivery.failed` entry in
> the log, or characters missing from a delivered sentence, is the signal to
> lower the batch size.

**FR-12.4** [Must] `[derived]` A partial delivery shall be reported as a failure.

> Most often UIPI: the foreground window belongs to an elevated process. Silence
> here looks like dictation losing half a sentence for no reason.

## 13. Window inspection

**FR-13.1** [Must] `[derived]` The process name and window title of the
foreground window shall be readable.

**FR-13.2** [Must] `[derived]` A window whose process has exited between handle
and query shall yield an empty descriptor, not an exception.

## 14. Feedback

**FR-14.1** [Must] `[user]` A tray icon shall show the current state and offer
quit, settings and recent-utterance recovery.

**FR-14.2** [Should] `[user]` An always-on-top overlay shall show *recording* and
*transcribing* at a fixed screen position, and shall be suppressible by
configuration.

> Default lower right of the main screen, above the taskbar. A fixed corner is
> somewhere the user can learn to glance at; an indicator that follows the
> cursor is never twice in the same place.

**FR-14.5** [Must] `[user]` Notifications shall be raised only when the user
needs to act or something went wrong.

> There are no per-utterance status popups: a balloon after every successful dictation is noise, and noise trains people to
> ignore the balloons that matter. Routine timings belong in the diagnostic log.

**FR-14.3** [Won't] `[user]` dictate makes no sound.

> The overlay and the tray icon carry the state, and audio cues cost more than
> they are worth. Tones mean a second audio device, and an idle audio endpoint
> takes seconds to spin back up: opening an output stream stalls capture while
> it does, so the first dictation after a pause would wait seconds for its own
> microphone. Holding the device open instead makes dictate sit on the output
> endpoint permanently, which interferes with everything else that uses it.
> dictate opens exactly one audio device, the microphone.

**FR-14.4** [Must] `[derived]` The overlay shall never take keyboard focus.

> An overlay that steals focus would break the pinned-target contract (FR-6.1)
> every single time.

---

# Part C — Foundation (L0)

## 15. Credential storage

**FR-15.1** [Must] `[user]` API keys shall be stored in Windows Credential
Manager as generic credentials, and shall not be written to any file.

**FR-15.2** [Must] `[derived]` `dictate --auth` shall prompt for and store the
keys the configured providers need (FR-11.13).

**FR-15.3** [Must] `[derived]` Keys shall not appear in logs, notifications, the
tray UI, or error messages.

**FR-15.4** [Must] `[derived]` Unmanaged buffers holding a key shall be zeroed
before being freed.

## 16. Configuration

**FR-16.1** [Must] `[derived]` Configuration shall be read from
`%APPDATA%\dictate\config.json`; a missing file shall yield documented defaults.

**FR-16.2** [Must] `[derived]` A malformed configuration file shall be a startup
error, not a silent fallback to defaults.

> A typo that silently reverts every setting looks exactly like the settings
> never having applied.

**FR-16.3** [Must] `[user]` The tray menu shall offer **Start with Windows**,
off by default. Ticking it shall make dictate start, unelevated, at the
current user's next login; unticking it shall stop that. The check mark shall
reflect every route that starts dictate at login — its own registration and a
dictate shortcut in the user's Startup folder — and a "Disabled" set in Task
Manager.

> Off by default: a program should not add itself to
> startup unasked. Per-user registration needs no elevation, which matters
> because an elevated dictate cannot type into ordinary windows. Ticking it
> when a Startup-folder shortcut already starts dictate adds nothing, so it
> never starts twice; unticking removes both. Ticking it again after moving the
> exe repairs the path. *Must not:* the check mark showing on while nothing
> starts dictate, or off while something does. *Tier:* desktop.

## 17. Retention

**FR-17.1** [Must] `[user]` No audio and no transcript shall be written to
persistent storage at any point.

> **VC** — *Pre:* a clean profile, file-system auditing on `%APPDATA%`, `%TEMP%`
> and the working directory. *Stimulus:* run ten utterances including a failing
> one. *Expect:* zero files created containing audio or transcript text.
> *Must not:* a crash dump or trace file containing utterance text. *Tier:*
> desktop. *Evidence:* audit log.

**FR-17.2** [Must] `[user]` Recent utterances shall be held in memory only and
lost when the process exits.

---

# Part D — Cross-cutting

## 18. Security profile

**Assets.** Two API keys; the content of everything the user dictates.

**Attacker.** Someone with file-system access to the user's profile — a stolen
laptop, malware running as the user, or a backup that leaves the machine.
**Not** in the model: an attacker with kernel privileges or a debugger attached
to the running process. Against those, an unelevated desktop application holding
keys in memory has no defence, and claiming otherwise would be theatre.

**Derived.** FR-15.1 (DPAPI, no key file) and FR-17.1 (no dictation content at
rest) between them mean a stolen profile yields neither the keys nor the history.

**Accepted, not mitigated.** Utterance audio and text traverse third-party
services in cleartext-to-them form: ElevenLabs and Anthropic by default, or
Infomaniak in Switzerland for either or both (§1.4). Infomaniak's asynchronous
API keeps the audio on its servers until the result is collected, for a period
it does not document. Anyone for whom that is unacceptable needs a local model,
which is a different product.

## 19. Performance

**NFR-19.1** [Should] `[user]` The median interval between hotkey release and
text appearing shall be under 2.5 s for a 10-second utterance on a domestic
connection.

> If this ever stops holding, streaming transcription is the lever — Harness §0.

**NFR-19.2** [Must] `[derived]` Neither network call shall block the UI thread.

**NFR-19.6** [Must] `[derived]` The application shall hold no audio device other
than the microphone.

> An idle audio endpoint takes seconds to spin back up, and opening any stream
> on it stalls a live capture stream while that happens — so a second device
> costs the start of the utterance, not merely the latency of whatever the second
> device was for. Both audio APIs pay it, so this is a property of the endpoint
> rather than of an API, and it cannot be scheduled around. Holding the second
> device open avoids the stall by sitting on the endpoint permanently, which
> interferes with every other application using it. The way out is not to have a
> second device (FR-14.3).
>
> **VC** — *Pre:* dictate idle for several minutes. *Stimulus:* press and hold.
> *Expect:* `mic.firstBuffer` under 250 ms. *Must not:* the first dictation after
> a pause being slower than one following a previous dictation. *Tier:* desktop.

**NFR-19.5** [Must] `[derived]` No audio-device open shall happen on the UI
thread.

> Opening an audio device is unbounded — seconds, on an endpoint that has gone
> idle — and the UI thread is where the session state machine runs. Hotkey events
> reach it through `BeginInvoke`, so a block there stops the *release* being
> processed: the user holds the key, lets go, and nothing happens until a device
> has finished opening. This covers the pre-warm as well as the session path;
> both open the microphone, and neither may do it on the UI thread.
>
> **VC** — *Pre:* dictate idle for over a minute. *Stimulus:* press and hold.
> *Expect:* `recording.start` is written within 250 ms of the press — compare its
> timestamp against `held` on the matching `recording.stop`. *Tier:* desktop.
> *Evidence:* `mic.firstBuffer` records what capture cost.

**NFR-19.3** [Should] `[derived]` Idle memory shall stay under 100 MB.

**NFR-19.4** [Must] `[derived]` Memory shall not grow across sessions beyond the
fixed recent-utterance buffer.

---

# Part E — Operations and verification

## 20. Build and release

**FR-20.1** [Must] `[user]` The workspace shall build and its host-tier tests run
on `ubuntu-latest`.

**FR-20.2** [Must] `[user]` A tagged commit shall publish a self-contained
single-file `dictate.exe` for `win-x64` as a release asset.

**FR-20.3** [Must] `[derived]` `dictate-core` shall not reference any
platform-specific API.

> **VC** — *Stimulus:* `cargo test --workspace` on `ubuntu-latest`. *Expect:*
> build and all host tests pass. *Must not:* a platform-specific dependency
> creeping into `dictate-core` — it would silently move the testable half out
> of CI.
> *Tier:* ci. *Evidence:* workflow run.

**FR-20.4** [Must] `[derived]` The published exe shall run on a Windows 11 x64
machine with no runtime or redistributable installed.

> The C runtime is linked statically (`.cargo/config.toml`); without it a clean
> machine reports `vcruntime140.dll` missing.

**FR-20.5** [Won't] `[user]` The published exe is not code-signed.

> SmartScreen therefore warns on first run, and the user clicks through
> *More info → Run anyway*. A signing certificate is an annual cost for a tool
> with one operator; the warning is a one-time click per release.

## 21. Verification

Test declarations live in [`../../testing/test-plan.yaml`](../../testing/test-plan.yaml)
— one entry per test with its tier, the requirements it discharges, and what it
last produced. The traceability matrix is generated from that file and from
`@fsd` tags in the source; it is never hand-maintained here.

**A requirement is not verified because a test exists for it.** The lifecycle is
*specified → test designed → implementation mapped → test implemented → executed →
evidence captured → verified*, and most requirements in this document currently
sit at the second or third step. Run `/fsd-engineer audit` for the real position.

---

# Part F — macOS

## 22. macOS client

The macOS client runs the same session, delivery policy, pipeline and
configuration as the Windows client; every requirement in Parts A–E applies
unless this section replaces it. What follows is only where macOS differs.

**FR-22.1** [Must] `[user]` The hotkey shall be read from `HotkeyVirtualKey` and
mean the same physical key as on Windows: the Windows keys are Command, Alt is
Option. The default `0x5C` is therefore **Right Command**. Modifiers (Command,
Option, Ctrl, Shift, left or right) and F1–F20 are supported; any other value
is a startup error naming the supported keys.

> The user types on a Windows keyboard on both machines, so one config.json and
> one finger position serve both. Right Command alone does nothing on macOS,
> which is the property that makes Right Windows the default (§1.4).

**FR-22.2** [Must] `[derived]` The hotkey shall be observed, never swallowed.
FR-8.4 (`SuppressHotkey`) and FR-8.8 do not apply on macOS.

> Right Command released with nothing pressed in between has no effect, so
> there is nothing to suppress, and an observing tap needs only Input
> Monitoring rather than control over the keyboard.

**FR-22.3** [Must] `[derived]` A press whose release is never delivered shall be
released once the hardware has read the key as up three times running, 100 ms
apart, and logged with `reason=Reconciled`. A key still held shall never be
released. A modifier is read by its own side's device flag, an F-key by its key
code.

> macOS stops delivering keystrokes to observers while a password field has
> secure input, and disables an observer it considers slow; either can swallow
> the release. This is FR-8.7's purpose, with the bound set by the watchdog
> rather than the keyboard repeat rate.
>
> The hardware key state reports both sides of a modifier under one key code:
> Right Ctrl held reads as key 59 (Left Ctrl) down and key 62 up. Only the
> device flag (`0x2000` for Right Ctrl) tells the sides apart, and it is the
> same bit the press itself is recognised by.
>
> **VC** — *Pre:* `Idle`, `HotkeyVirtualKey` set to Right Ctrl (`163`).
> *Stimulus:* hold the hotkey for 3 s and speak. *Expect:* exactly one session,
> `held≈3000ms`, `reason=Released`. *Must not:* `reason=Reconciled` while the
> key is held. *Tier:* desktop-mac.

**FR-22.4** [Must] `[derived]` A press is recorded as `injected` when its event
names a source process. dictate's own keystrokes carry a signature and are
never read as hotkey events (FR-8.3).

**FR-22.5** [Must] `[derived]` Capture shall use the input device's own format
and convert it to the 16 kHz mono PCM16 of FR-9.1, averaging the channels and
each output sample's input period. With `InputDevice` unset or unmatched, the
system default input is used (FR-9.6).

> The averaging keeps content above 8 kHz from folding into the speech band, at
> no cost.

**FR-22.6** [Must] `[derived]` Text shall be typed as keyboard events carrying
Unicode strings of at most 20 UTF-16 units, never splitting a surrogate pair,
with no modifier flags; a line break shall be typed as the Return key.
Without the Accessibility permission, typing shall fail with a message naming
the switch to turn on, and the text goes to the clipboard (FR-6.3).

> macOS discards keystrokes posted by an application without Accessibility and
> reports nothing; checking first is what turns a silent loss into FR-6.3's
> clipboard fallback.

**FR-22.7** [Must] `[derived]` The pinned target (FR-6.1) shall be the frontmost
on-screen application window: its process and window number. A console is an
application named in the built-in terminal list (Terminal, iTerm2, Warp,
Ghostty, kitty, Alacritty, WezTerm) or in `ExtraConsoleProcesses`. The window
title shall be read through Accessibility, and is empty without it.

**FR-22.8** [Must] `[user]` FR-14.1's tray icon is a menu-bar item: a coloured
dot per state, and a menu with **Recent**, **Microphone**, **Open config
folder**, **Re-enter API keys…** and **Quit dictate**. Notifications appear in
Notification Center. dictate has no Dock icon and no overlay on macOS.

**FR-22.9** [Must] `[user]` API keys shall be stored in the login Keychain as
generic passwords, service `dictate`, accounts `elevenlabs`, `anthropic` and `infomaniak`,
and shall not be written to any file. They are entered in system dialogs with
hidden input: at first start, through **Re-enter API keys…**, or with
`dictate --auth`. FR-15.3 applies unchanged.

**FR-22.10** [Must] `[derived]` Configuration is read from
`~/Library/Application Support/dictate/config.json` and the diagnostic log is
written to `~/Library/Logs/dictate/dictate.log`. FR-16.2 applies unchanged.

**FR-22.11** [Must] `[derived]` dictate shall ask for each permission when it
first needs it: Microphone at the first recording, Accessibility at startup,
Input Monitoring when the hotkey observer cannot start. Without Input
Monitoring, a notification says where to allow it, and dictate starts listening
once it is allowed. The permissions held are logged at startup
(`permissions`).

**FR-22.12** [Must] `[derived]` FR-5.4 holds through a lock file in the
configuration folder; the system releases it when dictate exits, however it
exits.

**FR-22.13** [Must] `[user]` A tagged commit shall also publish
`dictate-macos.zip`: `dictate.app`, one universal binary for Apple Silicon and
Intel, signed with the Developer ID and notarized. The signing certificate and
the notarization key exist only as GitHub Actions secrets. A build without them
is signed ad hoc and runs, but macOS asks for its permissions again after every
such build.

> FR-20.5 is Windows-only: a notarized application opens without a warning,
> and an unsigned one downloaded from the web does not open at all until the
> user overrides Gatekeeper.

**FR-22.14** [Won't] `[derived]` dictate offers no start-at-login option on
macOS; **System Settings → General → Login Items** starts it at login.

---

## Appendix A — Configuration catalogue

| Key | Type | Default | Requirement |
|---|---|---|---|
| `HotkeyVirtualKey` | int | `0x5C` (Right Windows; Right Command on macOS) | FR-8.1, FR-22.1 |
| `SuppressHotkey` | bool | `true` (no effect on macOS) | FR-8.4 |
| `IgnoreInjectedHotkey` | bool | `false` | FR-8.6 |
| `ShortPressPassesThrough` | bool | `false` | FR-8.9 |
| `MinimumHoldMs` | int | `300` | FR-5.2 |
| `MaximumRecordingSeconds` | int | `120` | FR-5.3 |
| `Language` | enum | `Auto` | FR-10.3, FR-10.8 |
| `SwissSpelling` | bool | `true` | FR-11.11 |
| `TranscriptionProvider` | enum | `ElevenLabs` (or `Infomaniak`) | FR-10.1 |
| `TranscriptionModel` | string? | `null`: `ScribeModelId` / `whisper` | FR-10.2 |
| `ScribeModelId` | string | `scribe_v2` | FR-10.2 |
| `CleanupProvider` | enum | `Anthropic` (or `Infomaniak`) | FR-11.1 |
| `InfomaniakProductId` | int? | `null`: the token's first product | FR-10.7 |
| `CleanupModel` | string? | `null`: `claude-haiku-4-5` / `google/gemma-4-31B-it` | FR-11.2 |
| `ScribeTimeoutSeconds` | int | `15` | FR-10.6 |
| `CleanupTimeoutSeconds` | int | `10` | FR-11.9 |
| `Vocabulary` | string[] | project terms | FR-11.6 |
| `ExtraCleanupInstruction` | string? | `null` | — |
| `ExtraConsoleProcesses` | string[] | `[]` | FR-6.5 |
| `AlwaysCopyToClipboard` | bool | `true` | FR-6.7 |
| `AppendSpaceAfterInsert` | bool | `true` | FR-6.4 |
| `SilenceThreshold` | int | `200` | FR-9.3 |
| `KeepMicrophoneOpen` | bool | `false` | FR-9.2 |
| `InputDevice` | string? | `null` (the system default input) | FR-9.6 |
| `EnableDiagnosticLog` | bool | `false` | — |
| `OverlayPosition` | enum | `BottomRight` | FR-14.2 |
| `InjectionChunkSize` | int | `200` | FR-12.3 |
| `InjectionChunkDelayMs` | int | `0` | FR-12.3 |
| `ShowOverlay` | bool | `true` | FR-14.2 |

## Appendix B — Glossary

| Term | Meaning |
|---|---|
| **Utterance** | One hotkey press-to-release cycle and everything derived from it |
| **Pinned target** | The foreground window recorded at hotkey-down (FR-6.1) |
| **Console target** | A window classified per FR-6.5, where newlines are forbidden |
| **Degraded delivery** | Raw transcript delivered because cleanup failed (FR-7.1) |
| **Host tier** | Tests running on a Linux CI runner against `dictate-core` |
| **Desktop tier** | Tests requiring the real Windows machine, run by hand |

## Related

- [[Harness/00-Overview]] — how the project is built and changed
- [[UserDocumentation/Manual]] — how to install and run it
