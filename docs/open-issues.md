# Open issues

Questions the FSD could not settle. Each names who can answer it and what is
blocked until they do. Items marked `[assumed]` in the FSD are being acted on in
the meantime — that is the point of the tag, not an excuse to leave it unresolved.

| # | Question | Who answers | Blocks | Current assumption |
|---|---|---|---|---|
| O-01 | Is the standard journey in FSD §1.3 the run you actually expect? | User | Workflow test W-01 — the acceptance gate | The eight steps as written |
| O-02 | Should the hotkey be swallowed, or passed through to the focused app? | User | FR-8.4 | Passed through. Right Ctrl alone is a no-op nearly everywhere, and swallowing it breaks Ctrl+key combinations |
| O-04 | Is 2.5 s an acceptable ceiling for release-to-text? | User, after using it | NFR-19.1 | 2.5 s. If it feels slow, streaming transcription is the lever — Harness §0 |
| O-05 | Should there be a second hotkey to pin the language for the next utterance? | User | An addition to FR-10.3 | No extra key; the config setting is enough |
| O-06 | Do any of the user's regular applications drop fast synthetic input? | Observation | The `InjectionChunkSize` default | None. 200 characters per batch, no delay |
| O-07 | Does the exe need code signing to avoid SmartScreen friction? | User | Release workflow | Unsigned. The warning is a one-time click |
| O-10 | Does anything on the machine synthesise the hotkey? | Observation | Whether `IgnoreInjectedHotkey` should default on (FR-8.6) | Nothing does. One session on 2026-08-17 at 21:15 was not started by the user, but it predates the origin instrumentation by 35 minutes, so its source cannot be recovered. Every press logged since has been `injected=False` |

## Resolved

| # | Question | Answer | Date |
|---|---|---|---|
| O-03 | Is `scribe_v2` the correct Scribe model identifier? | Yes. `scribe_v2` confirmed by the user; it stays the default for `ScribeModelId` (FR-10.2), and remains configurable so a future generation needs no rebuild. | 2026-08-16 |
| O-09 | Do handles, threads or working set grow per utterance? | No — plateau. Measured over nine utterances on the Windows machine: handles oscillate in a 709–753 band with no trend, threads sit near their starting 21, working set is flat around 91 MB. The early 45→97 MB reading was pool and JIT warm-up settling, as suspected. `KeepMicrophoneOpen` is **not** needed to hold it flat. | 2026-08-16 |

### O-08 — resolved: it was dictate, not ducking

Kept in full because the reasoning was wrong twice before the measurement
settled it, and the wrong version is instructive.

**What was believed at the time, and was wrong:**

Windows ducks other audio by 80% when it detects "communications activity". The
machine-side cause is `UserDuckingPreference` being unset, i.e. the default. The
suggested application-side fix — do not open capture under the `eCommunications`
role — assumes dictate opts into that role. **It does not.** Capture goes through
winmm (`WaveInEvent`), which exposes no role or category parameter at all; the
role is chosen for us.

Gaining that control means moving to WASAPI, and WASAPI shared mode hands back
the device's own mix format — 48 kHz stereo float on this machine — so it also
means adding a resampler to the hot path. That is a real change to the one part
of the pipeline currently working well, in exchange for politeness toward
background audio.

**None of that was the cause.** Disabling ducking system-wide on the Windows
machine did not remove the interference. The actual cause was dictate itself:
the feedback tones held a `WaveOutEvent` open for the entire life of the
process, playing silence, so dictate sat on the output endpoint around the
clock and stopped it idling. Closing the device after 30 seconds of quiet
(v0.1.6) removed the interference.

Two lessons worth keeping. The mechanism nobody proposed — an application
holding an idle output stream — beat two well-argued theories about Windows
audio internals. And the fix for a symptom that looked like a platform problem
was four lines in our own code.

The WASAPI question is therefore **closed, not deferred**: there was never a
ducking problem to solve.

**Postscript, 2026-08-17.** The fix above had a cost nobody looked for. Closing
the output device after 30 seconds means re-opening it on the next dictation —
and on this machine that open takes 3.3 seconds, runs on the UI thread, and
stalls the *capture* stream while it does. Measured: `mic.firstBuffer` at 3242 ms
against `feedback.open` at 3225 ms, eleven milliseconds apart. A 3.9-second
utterance kept 0.7 seconds of audio and was transcribed as Serbian.

So the same trade appears twice with opposite answers: holding the device open
interferes with other audio, closing it eagerly eats the start of every
utterance. 0.1.15 settles it by removing the reason to choose — the device is
warmed at startup, tones never touch the UI thread (NFR-19.5), and the timeout
is five minutes rather than thirty seconds. The third lesson to keep: a fix
measured only against the symptom it targeted can pay for itself somewhere the
measurement was not pointed.

When an item is answered, move it here with the answer and the date, and update
the FSD tag from `[assumed]` to `[user]` in the same commit.
