# dictate — User Manual

> **Status: incomplete.** The client is still being implemented, so the chapters
> below describe intended behaviour rather than something you can run today.
> Chapter 6 (Diagnostics) is a stub. This file ships from the start, carrying an
> honest status line, so that the first person to install dictate does not
> discover there are no instructions exactly when they need them.

The OPERATE plane. One manual with chapters — add a chapter, never a sibling
document.

## 1. What it does

Hold a key, speak, release. What you said is typed into whatever window you were
in, tidied up: fillers removed, punctuation added, your own vocabulary spelled
correctly. German and English, detected automatically.

Nothing is saved. No audio file, no transcript, no history on disk.

## 2. Installation

1. Download `dictate.exe` from the
   [latest release](https://github.com/SensorsIot/dictate/releases). It is
   self-contained — you do not need to install .NET.
2. Put it somewhere permanent, e.g. `%LOCALAPPDATA%\dictate\dictate.exe`.
   Windows SmartScreen will warn about an unsigned executable the first time;
   *More info → Run anyway*.
3. Store your API keys:

   ```
   dictate.exe --auth
   ```

   You will be asked for an **ElevenLabs** key (speech-to-text) and an
   **Anthropic** key (cleanup). They are written to Windows Credential Manager,
   encrypted under your Windows account. They are never written to a file.

4. To start dictate at login, put a shortcut to it in the folder that opens when
   you run `shell:startup`.

**Do not run dictate as administrator.** It will appear to work and then
silently fail to type into ordinary applications — Windows does not let an
elevated process send keystrokes to unelevated windows.

## 3. Everyday use

Hold **Right Ctrl**, speak, release.

| What you see or hear | What is happening |
|---|---|
| Rising beep, overlay says *recording* | Listening |
| Falling beep, overlay says *transcribing* | Uploading and cleaning up |
| Overlay clears, text appears | Done |
| Error buzz, notification | Something failed — the notification says what |

Two rules worth knowing:

- **Don't switch windows while it is thinking.** If the focused window changes
  between releasing the key and the text being ready, dictate puts the text on
  the clipboard and tells you, rather than typing it into whatever is now in
  front. Press Ctrl+V where you actually wanted it.
- **In a terminal you always get a single line.** Line breaks are stripped,
  because a newline typed into a shell is the Enter key and would run the
  command.

Right Ctrl still works as a modifier for shortcuts; dictate watches it without
swallowing it.

## 4. Configuration

`%APPDATA%\dictate\config.json`. Missing file means defaults. Edit it and restart
dictate.

The settings you are most likely to want:

| Setting | Default | What it does |
|---|---|---|
| `Language` | `Auto` | `German` or `English` pins the language when auto-detection keeps guessing wrong on short phrases |
| `Vocabulary` | project terms | Words to spell exactly this way — project names, call signs, jargon |
| `HotkeyVirtualKey` | `163` (Right Ctrl) | Virtual-key code to hold. **Do not use Right Alt** — on a Swiss keyboard that is AltGr, and you would lose `@ { } [ ] \` |
| `MinimumHoldMs` | `300` | Presses shorter than this are ignored as accidental taps |
| `MaximumRecordingSeconds` | `120` | Hard stop, so a stuck key cannot upload your afternoon |
| `AlwaysCopyToClipboard` | `true` | Also put each utterance on the clipboard, so a misdirected one is one Ctrl+V away |
| `PlaySounds` / `ShowOverlay` | `true` | Turn the feedback off |
| `EnableDiagnosticLog` | `false` | Write `%LOCALAPPDATA%\dictate\dictate.log` — timings and outcomes, never your text |
| `KeepMicrophoneOpen` | `false` | Hold the capture device open. Removes device-open latency, but lights the Windows microphone indicator permanently |
| `ExtraConsoleProcesses` | `[]` | Extra terminal executables that must never receive newlines |

A malformed `config.json` stops dictate at startup with an error rather than
silently reverting to defaults — a typo that quietly undid every setting would
look exactly like the settings never applying.

## 5. When it lands in the wrong place

**Just press Ctrl+V.** Every utterance goes on the clipboard as well as being
typed, so if it went somewhere you didn't intend — usually because the wrong
window had focus when you started — you can paste it where you actually wanted
it without re-dictating.

The trade-off: dictating overwrites whatever you had copied. If you routinely
dictate while holding something in the clipboard, set
`"AlwaysCopyToClipboard": false` and use the tray menu instead.

The tray menu keeps the last five utterances in memory and lets you copy any of
them again — useful when you notice two dictations later. They are gone when you
quit dictate; that is the no-persistence promise, not a bug.

## 6. Diagnostics and recovery

> **Stub.** To be written once the client is running and real failure modes are
> known. It will cover: no text appearing, wrong language, mangled terms, an
> unresponsive hotkey, and rotating a leaked API key.

### Music and other audio go quiet while you dictate

Windows, not dictate. When it detects "communications activity" it ducks all
other audio by 80%, and opening a microphone counts. Turn it off once:

1. Right-click the speaker icon → **Sound settings** → **More sound settings**
2. **Communications** tab → **Do nothing** → OK

dictate cannot switch this off for you. Capture goes through winmm, which does
not let an application choose its audio role — Windows picks it. Avoiding the
duck from inside dictate would mean moving to WASAPI and adding a resampler to
the recording path, which is a lot of new machinery in the one part of the
pipeline currently working well. If the ducking turns out to be a daily
irritation rather than a curiosity, say so and it is worth doing.

### Diagnostics

Set `"EnableDiagnosticLog": true` in the config and restart. dictate then writes
`%LOCALAPPDATA%\dictate\dictate.log` — one line per utterance with timings,
outcome, target application, and process counters:

```
2026-08-16 18:22:07.431  utterance  n=8  held=3200ms  status=Ok  chars=214
                         target=msedge  console=False  transcribe=780ms
                         cleanup=1100ms  wsMB=94  handles=722  threads=27
```

**It never records what you dictated** — only how long it was. That is
deliberate: the no-persistence promise stands, and a character count is enough
to correlate a delivery problem without keeping your sentences.

Known so far:

| Symptom | Likely cause |
|---|---|
| Nothing typed, no error | The focused window is running as administrator; dictate cannot send it input |
| Other audio ducks while dictating | Windows communications ducking — see above |
| Text goes to the clipboard every time | You are switching windows before the text is ready |
| Every utterance fails with a 4xx | Key is wrong or expired — re-run `dictate.exe --auth` |
| Every utterance fails mentioning a model | ElevenLabs changed the Scribe model identifier; set `ScribeModelId` in the config |
