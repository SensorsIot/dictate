# dictate — Functional Specification

| | |
|---|---|
| **Status** | Draft — requirements approved, verification largely undesigned |
| **Version** | 0.1.0 |
| **Last updated** | 2026-08-16 |
| **Derives from** | [`../decisions.md`](../decisions.md) (interview of 2026-08-16) |
| **Complexity** | Medium |
| **Plane** | WHAT — externally observable behaviour only. Build and code rules live in [`../Harness/`](../Harness/00-Overview.md); operating instructions in [`../UserDocumentation/Manual.md`](../UserDocumentation/Manual.md). |

Provenance tags: `[user]` decided by the user · `[derived]` follows necessarily
from a `[user]` decision · `[assumed]` inferred by the assistant and **not yet
confirmed** · `[code]` read back from the implementation.

---

## 1. Overview

### 1.1 Purpose

dictate turns speech into typed text on a Windows PC. The user holds a key,
speaks, and releases; a cleaned-up version of what they said is typed into
whichever window had focus when they started.

It exists because dictating prose into a text field is faster than typing it, and
because a verbatim speech-to-text transcript — fillers, false starts, no
punctuation, technical terms mangled — is not something anyone wants to send.

### 1.2 Users and context

One user, on one Windows 11 machine, dictating into a mix of terminals, editors,
browsers and chat clients. Speech is German, English, or both in the same
sentence. The vocabulary includes project names and radio call signs that no
general speech model spells correctly.

### 1.3 What "done" looks like — the standard journey `[assumed]`

The FSD is a set of clauses; none of them describes an ordinary successful run.
This is that run, and it is the acceptance gate. **Not yet confirmed by the user
— confirm before building the workflow test W-01.**

| # | Step | Observable |
|---|---|---|
| 1 | User launches `dictate.exe` | Tray icon appears, state *idle* |
| 2 | User places the caret in an editor | — |
| 3 | User presses and holds Right Ctrl | Start beep; overlay shows *recording* within 150 ms |
| 4 | User speaks one German sentence containing a vocabulary term | Overlay stays *recording* |
| 5 | User releases Right Ctrl | Stop beep; overlay shows *transcribing* |
| 6 | Pipeline runs | Overlay clears within 2.5 s |
| 7 | — | The sentence appears at the caret: punctuated, no fillers, in German, vocabulary term spelled per the configured list |
| 8 | User checks the disk | No audio file, no transcript file, anywhere |

### 1.4 Scope boundary

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
Right Ctrl down ──► pin target window ──► start capture (16 kHz mono PCM)
Right Ctrl up   ──► stop capture ──► WAV ──► Scribe ──► raw transcript
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
| **L2** Application logic | Dictation session state machine · delivery policy · degradation policy | `Dictate.Core` + `Dictate.Windows` |
| **L1** Interfaces | `ScribeTranscriber` · `HaikuCleaner` · `CleanupPrompt` · `TextSanitizer` · `Wav` · `AudioRecorder` · `HotkeyListener` · `TextInjector` · `WindowInspector` | mixed |
| **L0** Foundation | `HttpClient` · Anthropic SDK · NAudio/winmm · Credential Manager · configuration file | mixed |

**The layer split is not the project split**, and conflating them is the mistake
to avoid: `Dictate.Core` holds every layer's *platform-free* part, and
`Dictate.Windows` every layer's *Win32* part. `Dictate.Core` shall not reference
any Windows-only API (FR-21.3) — that is what keeps L1 and L2 testable on a Linux
runner.

### 2.4 Test architecture

| Tier | Runs where | Covers | Cost |
|---|---|---|---|
| **host** | `ubuntu-latest` in CI, every push | Everything in `Dictate.Core`: prompt construction, WAV framing, response parsing, sanitisation, pipeline degradation | Seconds |
| **desktop** | The user's Windows 11 machine, manually | Hotkey capture, microphone, `SendInput`, focus pinning, tray and overlay, Credential Manager | Minutes, human in the loop |
| **ci** | `windows-latest` in CI, on tag | The solution compiles and publishes a self-contained exe | Minutes |

Nothing in the **desktop** tier can be automated in this project's
infrastructure: it needs a real keyboard hook, a real microphone and a real
foreground window. Those requirements are verified by a scripted manual
procedure, and the FSD says so rather than implying automation that does not
exist.

---

## 3. Phases

| Phase | Contents | Exit gate |
|---|---|---|
| **P1 Core** | `Dictate.Core` complete, host tier green | `dotnet test` passes in CI |
| **P2 Client** | Hotkey, capture, injection, tray; end-to-end on the Windows machine | Journey (§1.3) passes manually |
| **P3 Hardening** | Focus-change fallback, degradation paths, limits | Deviation and negative tests pass |
| **P4 Release** | Tagged build publishing a self-contained exe | A downloaded exe runs on a machine with no .NET installed |

---

## 4. Risks

| ID | Risk | Consequence | Mitigation |
|---|---|---|---|
| R-01 | Text is typed into the wrong window after an alt-tab | A private sentence lands in the wrong chat, or a shell | FR-7.1 pins the target; FR-7.2 falls back to the clipboard |
| R-02 | A newline is typed into a terminal | Whatever is on the command line executes | FR-7.4 forbids it structurally, not by prompting |
| R-03 | Cleanup rewrites meaning or translates | The user sends something they did not say | FR-12.4/FR-12.5; cleanup never sees a request to answer |
| R-04 | Audio of everything spoken accumulates on disk | A laptop theft becomes a recording leak | FR-18.1: no audio or transcript is ever written |
| R-05 | An API key is readable on the machine | Account compromise | FR-16.1: DPAPI via Credential Manager, never a file |
| R-06 | A third-party app mishandles synthetic keystrokes | Dictation appears broken in that app | Accepted — outside the §1.4 boundary. Per-app chunking (FR-13.3) is the escape hatch |
| R-07 | CI-only builds mean compile errors surface late | Slow iteration | Accepted by the user (D-15). Host tests keep the blast radius small |
| R-08 | Scribe or Anthropic changes a model identifier | Every utterance fails | FR-11.2/FR-12.2 make both configurable; FR-8.4 surfaces the API's own error text |

### 4.1 Explicitly out of scope

- Voice commands ("delete that", "make it formal") — rejected in D-07.
- Streaming transcription — deferred in D-16.
- Any platform other than Windows 11 x64.
- Multi-user or shared-machine operation.
- Persisting transcripts, audio, or usage history (D-12).

---

# Part A — Application logic (L2)

## 5. Dictation session

### 5.1 State model (normative)

The transition table is normative; any diagram is generated from it. Every
(state × event) pair below is handled or explicitly ignored.

| From | Event | Guard | To | Action |
|---|---|---|---|---|
| `Idle` | hotkey down | input device present | `Recording` | Pin target window; start capture; start beep; show overlay |
| `Idle` | hotkey down | no input device | `Idle` | Error toast; error beep |
| `Recording` | hotkey up | held ≥ `MinimumHoldMs` | `Transcribing` | Stop capture; stop beep; overlay → *transcribing* |
| `Recording` | hotkey up | held < `MinimumHoldMs` | `Idle` | Discard audio; clear overlay; no sound |
| `Recording` | max duration reached | — | `Transcribing` | Stop capture; error beep; proceed with what was captured |
| `Recording` | hotkey down | — | `Recording` | Ignored (auto-repeat) |
| `Transcribing` | pipeline returns text | — | `Delivering` | — |
| `Transcribing` | pipeline returns failure | — | `Idle` | Error toast; error beep; clear overlay |
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

**FR-5.2** [Must] `[user]` A press held for less than `MinimumHoldMs`
(default 300 ms) shall be discarded without transcription and without sound.

> **VC** — *Pre:* `Idle`. *Stimulus:* tap the hotkey for < 300 ms. *Expect:* no
> network request; no beep; state returns to `Idle`. *Must not:* an empty
> utterance reaching Scribe (it is billable). *Tier:* desktop.

**FR-5.3** [Must] `[user]` Capture shall stop automatically after
`MaximumRecordingSeconds` (default 120 s) and the captured audio shall still be
processed.

> **VC** — *Pre:* `Recording`. *Stimulus:* hold past the limit. *Expect:* capture
> stops within 1 s of the limit; error beep; pipeline runs on what was captured.
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
configurable virtual-key code, default `0xA3` (Right Ctrl), globally — regardless
of which application has focus.

**FR-8.2** [Must] `[derived]` Keyboard auto-repeat shall not generate additional
press events.

**FR-8.3** [Must] `[derived]` Keystrokes synthesised by dictate itself shall not
be interpreted as hotkey events.

> Without this, dictating a sentence while the hotkey character is in the text
> re-triggers the session. Implemented by stamping `dwExtraInfo`.

**FR-8.4** [Should] `[assumed]` The hotkey shall be passed through to the focused
application by default, and suppressed only if the user opts in.

> Rationale: Right Ctrl alone is a no-op in nearly every application, whereas
> swallowing it breaks every Ctrl+key combination made with that hand. `[assumed]`
> — the user chose the key (D-17) but not the suppression behaviour.

**NFR-8.5** [Must] `[derived]` The application shall run unelevated.

> A low-level hook installed by an elevated process cannot see input destined for
> unelevated windows. Running as administrator would stop dictation working in
> ordinary applications.

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

## 10. Transcription

**FR-10.1** [Must] `[user]` Audio shall be transcribed by ElevenLabs Scribe via
`POST https://api.elevenlabs.io/v1/speech-to-text`, authenticated with the
`xi-api-key` header.

**FR-10.2** [Must] `[user]` The Scribe model identifier shall be configurable,
default `scribe_v2`.

> `scribe_v2` confirmed by the user on 2026-08-16 (O-03). It stays configurable
> so a future Scribe generation is a settings change, not a rebuild.

**FR-10.3** [Must] `[user]` When `Language` is `Auto` no `language_code` shall be
sent; when it is `German` or `English`, `de` or `en` shall be sent respectively.

**FR-10.4** [Must] `[derived]` `tag_audio_events` and `diarize` shall both be
sent as `false`.

> `(laughter)` and speaker labels are noise when the output goes straight into a
> text field.

**FR-10.5** [Must] `[derived]` A non-2xx response shall raise an error carrying
the response body.

**FR-10.6** [Must] `[derived]` The request shall be abandoned after
`ScribeTimeoutSeconds` (default 15 s).

> **VC** for FR-10.1/10.3/10.5 — *Pre:* a recorded Scribe response fixture.
> *Stimulus:* parse it. *Expect:* `text` and `language_code` extracted, whitespace
> trimmed; a response without `text` raises `TranscriptionException`. *Tier:* host
> (`ScribeParseTests`).

## 11. Cleanup

**FR-11.1** [Must] `[user]` The raw transcript shall be cleaned by Claude Haiku
4.5 through the Anthropic Messages API before delivery.

**FR-11.2** [Must] `[derived]` The cleanup model identifier shall be configurable,
default `claude-haiku-4-5`.

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

**FR-11.6** [Must] `[user]` Terms listed in `Vocabulary` shall be spelled as
configured.

**FR-11.7** [Should] `[user]` The process name and window title of the pinned
target shall be supplied to cleanup so it can adapt formatting.

**FR-11.8** [Must] `[derived]` Cleanup shall run at `temperature = 0`.

> The same utterance must not clean up two different ways.

**FR-11.9** [Must] `[derived]` The request shall be abandoned after
`CleanupTimeoutSeconds` (default 10 s), degrading per FR-7.1.

**FR-11.10** [Should] `[derived]` If no Anthropic key is configured, dictation
shall still work, delivering the verbatim transcript.

## 12. Text injection

**FR-12.1** [Must] `[user]` Text shall be delivered with `SendInput` using
Unicode scan codes (`KEYEVENTF_UNICODE`), not clipboard paste.

**FR-12.2** [Must] `[derived]` Characters outside the Basic Multilingual Plane
shall be delivered correctly as surrogate pairs.

**FR-12.3** [Should] `[derived]` Events shall be sent in batches of
`InjectionChunkSize` (default 200) with `InjectionChunkDelayMs` between batches.

> One `SendInput` call per paragraph rather than per character; the chunk size is
> the escape hatch for applications that drop fast input (R-06).

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
*transcribing*, and shall be suppressible by configuration.

**FR-14.3** [Should] `[user]` Distinct sounds shall mark start, stop and error,
and shall be suppressible by configuration.

**FR-14.4** [Must] `[derived]` The overlay shall never take keyboard focus.

> An overlay that steals focus would break the pinned-target contract (FR-6.1)
> every single time.

---

# Part C — Foundation (L0)

## 15. Credential storage

**FR-15.1** [Must] `[user]` API keys shall be stored in Windows Credential
Manager as generic credentials, and shall not be written to any file.

**FR-15.2** [Must] `[derived]` `dictate --auth` shall prompt for and store both
keys.

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

**Accepted, not mitigated.** Utterance audio and text traverse two third-party
services in cleartext-to-them form. That is inherent to D-02 and D-03. Anyone for
whom that is unacceptable needs a local model, which is a different product.

## 19. Performance

**NFR-19.1** [Should] `[assumed]` The median interval between hotkey release and
text appearing shall be under 2.5 s for a 10-second utterance on a domestic
connection.

**NFR-19.2** [Must] `[derived]` Neither network call shall block the UI thread.

**NFR-19.3** [Should] `[derived]` Idle memory shall stay under 100 MB.

**NFR-19.4** [Must] `[derived]` Memory shall not grow across sessions beyond the
fixed recent-utterance buffer.

---

# Part E — Operations and verification

## 20. Build and release

**FR-20.1** [Must] `[user]` The solution shall build and its host-tier tests run
on `ubuntu-latest`.

**FR-20.2** [Must] `[user]` A tagged commit shall publish a self-contained
single-file `dictate.exe` for `win-x64` as a release asset.

**FR-20.3** [Must] `[derived]` `Dictate.Core` shall not reference any Windows-only
API.

> **VC** — *Stimulus:* `dotnet test` on `ubuntu-latest`. *Expect:* restore, build
> and all host tests pass. *Must not:* a platform-specific dependency creeping
> into `Dictate.Core` — it would silently move the testable half out of CI.
> *Tier:* ci. *Evidence:* workflow run.

**FR-20.4** [Must] `[derived]` The published exe shall run on a Windows 11 x64
machine with no .NET runtime installed.

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

## Appendix A — Configuration catalogue

| Key | Type | Default | Requirement |
|---|---|---|---|
| `HotkeyVirtualKey` | int | `0xA3` (Right Ctrl) | FR-8.1 |
| `SuppressHotkey` | bool | `false` | FR-8.4 |
| `MinimumHoldMs` | int | `300` | FR-5.2 |
| `MaximumRecordingSeconds` | int | `120` | FR-5.3 |
| `Language` | enum | `Auto` | FR-10.3 |
| `ScribeModelId` | string | `scribe_v2` | FR-10.2 |
| `CleanupModel` | string | `claude-haiku-4-5` | FR-11.2 |
| `ScribeTimeoutSeconds` | int | `15` | FR-10.6 |
| `CleanupTimeoutSeconds` | int | `10` | FR-11.9 |
| `Vocabulary` | string[] | project terms | FR-11.6 |
| `ExtraCleanupInstruction` | string? | `null` | — |
| `ExtraConsoleProcesses` | string[] | `[]` | FR-6.5 |
| `AlwaysCopyToClipboard` | bool | `true` | FR-6.7 |
| `AppendSpaceAfterInsert` | bool | `true` | FR-6.4 |
| `SilenceThreshold` | int | `200` | FR-9.3 |
| `KeepMicrophoneOpen` | bool | `false` | FR-9.2 |
| `EnableDiagnosticLog` | bool | `false` | — |
| `ShowTimings` | bool | `false` | — |
| `InjectionChunkSize` | int | `200` | FR-12.3 |
| `InjectionChunkDelayMs` | int | `0` | FR-12.3 |
| `PlaySounds` | bool | `true` | FR-14.3 |
| `ShowOverlay` | bool | `true` | FR-14.2 |

## Appendix B — Glossary

| Term | Meaning |
|---|---|
| **Utterance** | One hotkey press-to-release cycle and everything derived from it |
| **Pinned target** | The foreground window recorded at hotkey-down (FR-6.1) |
| **Console target** | A window classified per FR-6.5, where newlines are forbidden |
| **Degraded delivery** | Raw transcript delivered because cleanup failed (FR-7.1) |
| **Host tier** | Tests running on a Linux CI runner against `Dictate.Core` |
| **Desktop tier** | Tests requiring the real Windows machine, run by hand |

## Related

- [[decisions]] — what was chosen and what was rejected
- [[Harness/00-Overview]] — how the project is built and changed
- [[UserDocumentation/Manual]] — how to install and run it
