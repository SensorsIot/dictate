# Open issues

Questions the FSD could not settle. Each names who can answer it and what is
blocked until they do. Items marked `[assumed]` in the FSD are being acted on in
the meantime — that is the point of the tag, not an excuse to leave it unresolved.

| # | Question | Who answers | Blocks | Current assumption |
|---|---|---|---|---|
| O-01 | Is the standard journey in FSD §1.3 the run you actually expect? | User | Workflow test W-01 — the acceptance gate | The eight steps as written |
| O-02 | Should the hotkey be swallowed, or passed through to the focused app? | User | FR-8.4 | Passed through. Right Ctrl alone is a no-op nearly everywhere, and swallowing it breaks Ctrl+key combinations |
| O-04 | Is 2.5 s an acceptable ceiling for release-to-text? | User, after using it | NFR-19.1 | 2.5 s. If it feels slow, D-16 (streaming) is the lever |
| O-05 | Should there be a second hotkey to pin the language for the next utterance? | User | An addition to FR-10.3 | No extra key; the config setting is enough |
| O-06 | Do any of the user's regular applications drop fast synthetic input? | Observation | The `InjectionChunkSize` default | None. 200 characters per batch, no delay |
| O-08 | Opening the microphone makes Windows duck all other audio by 80%. Can dictate avoid triggering it, or is this a machine setting? | Experiment on the Windows machine | Nothing — dictation works, it is just rude to whatever is playing | The user-side setting fixes it outright. Whether dictate can avoid provoking it is unproven — see below |
| O-07 | Does the exe need code signing to avoid SmartScreen friction? | User | Release workflow | Unsigned. The warning is a one-time click |

## Resolved

| # | Question | Answer | Date |
|---|---|---|---|
| O-03 | Is `scribe_v2` the correct Scribe model identifier? | Yes. `scribe_v2` confirmed by the user; it stays the default for `ScribeModelId` (FR-10.2), and remains configurable so a future generation needs no rebuild. | 2026-08-16 |
| O-09 | Do handles, threads or working set grow per utterance? | No — plateau. Measured over nine utterances on the Windows machine: handles oscillate in a 709–753 band with no trend, threads sit near their starting 21, working set is flat around 91 MB. The early 45→97 MB reading was pool and JIT warm-up settling, as suspected. `KeepMicrophoneOpen` is **not** needed to hold it flat. | 2026-08-16 |

### O-08 — what is actually known about ducking

Recorded here rather than acted on, because the obvious fix does not fit the
current audio stack.

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

So the honest position: the one-line user setting fixes it today
(Sound → Communications → *Do nothing*), and the WASAPI route stays open if
ducking turns out to be a daily irritation rather than a curiosity.

When an item is answered, move it here with the answer and the date, and update
the FSD tag from `[assumed]` to `[user]` in the same commit.
