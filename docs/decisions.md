# Decisions

Settled in the design interview of 2026-08-16, before any code was written. Every
entry here is `[user]` provenance: it was chosen by the user, not inferred by the
assistant. The FSD derives from these; where the FSD later disagrees, this file
is the record of what was actually decided and the FSD is wrong.

| # | Decision | Rejected alternatives | Why |
|---|---|---|---|
| D-01 | Dictation runs on **Windows 11**, the user's daily machine | Linux desktop; cross-platform; headless service on dev-1 | The hotkey, the microphone and the target window all live there. A Linux container has none of the three. |
| D-02 | Speech-to-text is **ElevenLabs Scribe** (cloud) | Local faster-whisper on GPU or CPU; Groq; Deepgram | Chosen outright by the user. dev-1 has 4 QEMU vCPUs and no GPU, so a local model on the VM was never viable. |
| D-03 | Cleanup is **Claude Haiku 4.5** | Gemini Flash (the user's first choice, changed mid-interview); no cleanup | Same latency and cost class as Flash for a short cleanup pass. |
| D-04 | Trigger is **push-to-talk, hold** | Toggle; toggle + silence auto-stop; both | Key release is an unambiguous end-of-utterance signal. No VAD, no way to leave the microphone hot. |
| D-05 | Delivery is **synthetic typing via `SendInput`** | Clipboard paste + restore; paste-long/type-short; clipboard only | Works in terminals and over RDP without a per-application paste chord, and never touches the clipboard. |
| D-06 | **German + English, auto-detected, with a manual pin** | English only; German only; full auto with no override | The user's work mixes both in one sentence. Auto-detect misfires on short utterances, hence the override. |
| D-07 | Cleanup does **hygiene + personal vocabulary + application context** | Hygiene only; hygiene + vocabulary; full assistant with voice commands | Voice commands were rejected: the model would have to separate content from instruction and will eventually eat a wanted sentence. |
| D-08 | **`Dictate.Core` (platform-free) + `Dictate.Windows` (all Win32), one repository** | Single flat package; two repositories; develop directly on Windows | Keeps the whole testable half runnable on a Linux CI runner. |
| D-09 | API keys live in **Windows Credential Manager** | Infisical CLI on Windows; plaintext `.env` in `%APPDATA%`; proxy through dev-1 | DPAPI-encrypted at rest, works off the home LAN, no dependency on dev-1 being up. |
| D-10 | On focus change: **pin the target window at key-press, fall back to the clipboard** | Best-effort typing; clipboard on any anomaly; strict + retry queue | Typing into whatever happens to be in front is how dictation pastes a sentence into the wrong chat, or a shell. |
| D-11 | Feedback is **tray icon + overlay + beeps** | Beeps only; tray only; a full application window | With a 1–2 s cloud round trip the user needs a "still working" signal, not just a start/stop cue. |
| D-12 | **Nothing is persisted to disk** — no audio, no transcripts | Transcripts kept with an audio ring buffer; everything retained; transcripts only | Chosen by the user against the assistant's recommendation. Consequence accepted in D-13. |
| D-13 | Recovery is an **in-process ring buffer of recent utterances**, lost on exit | Nothing at all | Reconciles D-12 with D-10: a clipboard fallback the user missed is still recoverable until they quit. |
| D-14 | Language is **C# / .NET 9** | Python; Rust; TypeScript/Electron | Challenged by the user after the assistant defaulted to Python. The application is Win32 interop plus two HTTP calls — exactly where Python is weakest and C# strongest. Ships as one self-contained exe. |
| D-15 | Builds happen **only in GitHub Actions**; no .NET SDK in the devcontainer | SDK in the container for the inner loop; build on Windows; container builds + CI releases | The assistant recommended the SDK locally and set out the cost; the user reaffirmed CI-only and accepted the round-trip latency. |
| D-16 | **Batch upload** after key release; streaming deferred | Streaming from the start; batch permanently | Correctness first, measure, then decide whether ~1.5 s actually feels slow. `ITranscriber` is the seam a streaming backend drops into. |
| D-17 | Hotkey is **Right Ctrl** | CapsLock; mouse side button; configurable-only | Right Alt is AltGr on the user's Swiss layout — binding it would cost `@ { } [ ] \`. |
| D-18 | Repository **`SensorsIot/dictate`, public**; devcontainer on host port **2227** | Private | Matches the rest of the fleet. No secrets in the tree. |

## Decided during field use, 2026-08-16

Everything above predates any code. These came out of running it on the real
machine, and each replaced something that had seemed reasonable on paper.

| # | Decision | What it replaced | Why |
|---|---|---|---|
| D-19 | Short synthesised tones (B5 start, D5 stop, low G3 error) | `SystemSounds` Asterisk / Beep / Hand | Those are Windows notification and error chimes. An error chime at the end of every sentence trains you to ignore it. |
| D-20 | The output device closes after 30 s of quiet | Holding it open for the process lifetime | Holding it open kept dictate on the loudspeaker around the clock and stopped the endpoint idling — the actual cause of the audio interference, after two theories about Windows ducking proved wrong. |
| D-21 | Every utterance also goes on the clipboard | Clipboard only when focus changed mid-flight | The focus-change check structurally cannot see the commoner case: the wrong window was focused the whole time, so nothing changed and delivery looked successful. |
| D-22 | The indicator sits in a fixed screen corner | Following the mouse cursor | A fixed corner is somewhere you learn to glance at; one that follows the cursor is never twice in the same place. |
| D-23 | An opt-in diagnostic log, metadata only | Nothing | Diagnosing a shipped binary from another machine meant reconstructing behaviour from Windows event logs, where a silent exit and a deliberate quit look identical. The log records timings and counters, never dictated text, so D-12 still holds. |
| D-24 | No notification on a successful dictation | A per-utterance timings popup | It was a testing aid. A balloon after every sentence is noise, and noise trains people to ignore the balloons that matter. |

## Assistant recommendations the user overrode

Recorded because they are the decisions most likely to be revisited, and the
reasoning should not have to be reconstructed.

- **D-12 (no persistence).** The assistant recommended keeping transcripts with a
  short audio ring buffer, for prompt tuning and undo. The user chose zero
  retention. D-13 is the compromise that keeps the "nothing is lost" half of D-10
  honest.
- **D-15 (CI-only builds).** The assistant recommended the .NET SDK in the
  container so the inner loop is seconds rather than a 2–3 minute CI round trip,
  and noted that the C# language server needs it for IntelliSense. The user
  accepted both costs explicitly.

## Open, deferred deliberately

- Streaming transcription (D-16) — revisit after measuring real latency.
- Whether the cleanup pass should be benchmarked against another model of the
  same tier. Not a blocker; the `ICleaner` seam makes it a one-class change.
