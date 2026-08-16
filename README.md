# dictate

**Hold a key, speak, release — what you said appears where you were typing.**

Not a transcript of what you said. What you *meant*: the "ähm" and the false start
removed, the punctuation added, your own vocabulary spelled correctly. German and
English, including both in the same sentence.

Nothing is saved. No audio file, no transcript, no history on disk.

```
Hold Right Ctrl ─▶ speak ─▶ release ─▶ ~1.5 s ─▶ text is typed at your caret
```

## How it works

Three stages, and dictate is mostly the glue between them.

1. **Listen.** The moment you press the key, dictate remembers *which window you
   were in* and starts recording. That memory matters more than it sounds —
   see below.
2. **Transcribe.** On release the audio goes to [ElevenLabs
   Scribe](https://elevenlabs.io/speech-to-text), which returns it verbatim:
   fillers, false starts, no punctuation.
3. **Clean up.** That transcript goes to [Claude Haiku
   4.5](https://www.anthropic.com/claude) with your personal vocabulary and the
   name of the app you're typing into. It fixes the transcript. It is explicitly
   forbidden to translate it, answer it, or improve on what you meant.

Then the text is typed into the window from step 1, one keystroke at a time, as
though you had typed it.

### The parts that exist because dictation goes wrong

Most of dictate is not the happy path.

- **It types into the window you *started* in.** If you alt-tab while it's
  thinking, the text goes to the clipboard and tells you — rather than appearing
  in whatever is now in front, which might be a different chat or a shell prompt.
- **It never types a newline into a terminal.** Enter in a shell runs the line.
  Line breaks are collapsed to spaces for console windows, structurally, not by
  asking the model nicely.
- **If cleanup fails, you get the raw transcript.** A scruffy sentence beats a
  lost one.
- **If the microphone produced silence, it says so** — instead of uploading
  three seconds of zeroes and reporting that nothing was recognised, which blames
  the speech model for a gain knob.
- **Every utterance also goes on the clipboard**, so one that lands in the wrong
  place is one Ctrl+V from the right one.

## Install

1. Download `dictate.exe` from the
   [latest release](https://github.com/SensorsIot/dictate/releases/latest). It is
   self-contained — no .NET runtime needed. Windows SmartScreen will warn about
   an unsigned executable: *More info → Run anyway*.
2. Store your API keys (they go into Windows Credential Manager, encrypted under
   your account, never into a file):

   ```
   dictate.exe --auth
   ```

   You need an **ElevenLabs** key. An **Anthropic** key is optional — without it
   you get dictation without cleanup.
3. Run it. It lives in the tray. To start it at login, put a shortcut in
   `shell:startup`.

**Don't run it as administrator.** It will appear to work and then silently fail
to type into ordinary applications — Windows does not let an elevated process
send keystrokes to unelevated windows.

Full instructions, settings and troubleshooting:
**[the user manual](docs/UserDocumentation/Manual.md)**.

## Built with

C# / .NET 9, shipped as one self-contained `win-x64` executable.

| Piece | What |
|---|---|
| `Dictate.Core` | Platform-free: the pipeline, prompt, sanitisation, degradation rules. Unit-tested on Linux in CI. |
| `Dictate.Windows` | Every Win32 call: `WH_KEYBOARD_LL` hook, winmm capture, `SendInput`, Credential Manager, tray and overlay. |
| [NAudio](https://github.com/naudio/NAudio) | Capture and the feedback tones |
| [Anthropic SDK](https://github.com/anthropics/anthropic-sdk-csharp) | The cleanup call |

The split is deliberate: it keeps the whole testable half runnable on a Linux CI
runner, and CI compiling `Dictate.Core` on ubuntu is what enforces it staying
that way.

## Documentation

| | |
|---|---|
| [What it must do](docs/Functionality/FSD.md) | Functional specification, state model, verification contracts |
| [How it is built](docs/Harness/00-Overview.md) | Build rules, seams, testing standard |
| [How to run it](docs/UserDocumentation/Manual.md) | Install, use, configure, recover |
| [Why it is like this](docs/decisions.md) | Every design decision and what was rejected |
| [What is verified](testing/test-plan.yaml) | Every test, its tier, and what it last produced |

## Status

Working and in daily use. 71 automated tests, all passing; the pipeline,
latency, memory behaviour and delivery paths have been measured on real
hardware.

Honest about what that does *not* cover: the desktop-tier tests are run by hand,
because a hotkey, a microphone and a foreground window are things no CI runner
has. [`testing/test-plan.yaml`](testing/test-plan.yaml) says exactly which
requirements are discharged by measurement and which are still only designed.

## Licence

MIT.
