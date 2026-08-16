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
| O-07 | Does the exe need code signing to avoid SmartScreen friction? | User | Release workflow | Unsigned. The warning is a one-time click |

## Resolved

| # | Question | Answer | Date |
|---|---|---|---|
| O-03 | Is `scribe_v2` the correct Scribe model identifier? | Yes. `scribe_v2` confirmed by the user; it stays the default for `ScribeModelId` (FR-10.2), and remains configurable so a future generation needs no rebuild. | 2026-08-16 |

When an item is answered, move it here with the answer and the date, and update
the FSD tag from `[assumed]` to `[user]` in the same commit.
