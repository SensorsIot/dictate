# dictate — User Manual

> Everything below describes behaviour that has been run on a real machine, not
> intended behaviour.

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

Hold **Right Windows**, speak, release.

| What you see | What is happening |
|---|---|
| Overlay says *recording* | Listening |
| Overlay says *transcribing* | Uploading and cleaning up |
| Overlay clears, text appears | Done |
| Notification | Something failed — the notification says what |

Two rules worth knowing:

- **Don't switch windows while it is thinking.** If the focused window changes
  between releasing the key and the text being ready, dictate puts the text on
  the clipboard and tells you, rather than typing it into whatever is now in
  front. Press Ctrl+V where you actually wanted it.
- **In a terminal you always get a single line.** Line breaks are stripped,
  because a newline typed into a shell is the Enter key and would run the
  command.

The hotkey is swallowed, so the right Windows key no longer opens the Start menu
or acts as a modifier. The left Windows key is untouched.

## 4. Configuration

`%APPDATA%\dictate\config.json`. Missing file means defaults. Edit it and restart
dictate.

The settings you are most likely to want:

| Setting | Default | What it does |
|---|---|---|
| `Language` | `Auto` | `German` or `English` pins the language when auto-detection keeps guessing wrong on short phrases |
| `Vocabulary` | project terms | Words to spell exactly this way — project names, call signs, jargon |
| `HotkeyVirtualKey` | `92` (Right Windows) | Virtual-key code to hold — see the table below for the ones worth using |
| `SuppressHotkey` | `true` | Swallow the hotkey instead of passing it to the focused window. Required for the Windows key, which otherwise opens the Start menu on every dictation. Turn it off if you move the hotkey to an already-inert key |
| `IgnoreInjectedHotkey` | `false` | Ignore hotkey presses synthesised by other software (mouse-button mappings, macro tools). Leave off if you dictate over AnyDesk, TeamViewer or RDP — those deliver your real keystrokes the same way |
| `MinimumHoldMs` | `300` | Presses shorter than this are ignored as accidental taps |
| `MaximumRecordingSeconds` | `120` | Hard stop, so a stuck key cannot upload your afternoon |
| `AlwaysCopyToClipboard` | `true` | Also put each utterance on the clipboard, so a misdirected one is one Ctrl+V away |
| `ShowOverlay` | `true` | The recording indicator. dictate makes no sound, so this is the only cue — turning it off leaves nothing to tell you a session is running |
| `OverlayPosition` | `BottomRight` | Where the recording indicator sits: `BottomRight`, `BottomLeft`, `TopRight`, `TopLeft`, or `NearCursor` to have it follow the mouse |
| `EnableDiagnosticLog` | `false` | Write `%LOCALAPPDATA%\dictate\dictate.log` — timings and outcomes, never your text |
| `KeepMicrophoneOpen` | `false` | Hold the capture device open. Removes device-open latency, but lights the Windows microphone indicator permanently |
| `ExtraConsoleProcesses` | `[]` | Extra terminal executables that must never receive newlines |

A malformed `config.json` stops dictate at startup with an error rather than
silently reverting to defaults — a typo that quietly undid every setting would
look exactly like the settings never applying.

### Choosing the hotkey

Left and right are separate keys with separate numbers. dictate watches exactly
the one you name, so `92` never fires on the left Windows key and `162` never
fires on the right Ctrl.

| Key | Number | Notes |
|---|---|---|
| Left Ctrl | `162` | Breaks Ctrl+C and every other left-hand chord if suppressed |
| Right Ctrl | `163` | Inert on its own, but it is also browser zoom — Ctrl+scroll would start a dictation every time you zoom |
| Left Windows | `91` | Needs `SuppressHotkey`. Also the key most Windows shortcuts are built on |
| **Right Windows** | **`92`** | The default. Needs `SuppressHotkey`, which is on by default. Nothing else competes for it |
| Left Alt | `164` | Opens the menu bar of the focused window on its own |
| Right Alt | `165` | **Do not use.** On a Swiss keyboard this is AltGr, and you would lose `@ { } [ ] \` |
| Scroll Lock | `145` | Genuinely unused on modern machines, but absent from many compact keyboards |

**A Windows key requires `"SuppressHotkey": true` in the same edit.** Pressing
and releasing either Windows key with nothing in between opens the Start menu, so
without suppression you get the Start menu on every dictation. Suppression stops
the key reaching the shell — at the price of every Win+*key* shortcut made with
that hand. On the right Windows key that price is usually zero.

Not every keyboard has a right Windows key; compact and some gaming models
replace it with `Fn`. If dictation stops responding after the change, that is the
first thing to check.

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

### Other audio misbehaves while dictate is running

dictate opens exactly one audio device, the microphone, and makes no sound of
its own — so it should not affect anything you are playing. If it does, that is
a bug worth reporting.

Windows does have its own feature that ducks all other audio by 80% when it
detects "communications activity", and opening a microphone counts. That is a
machine setting, not dictate: **Sound settings → More sound settings →
Communications → Do nothing**.

### Turning the indicator off

`"ShowOverlay": false` hides the
indicator. The tray icon still changes colour, so you keep a state cue either
way.

### The diagnostic log

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

### Symptoms and causes

Turn the diagnostic log on first — it answers most of these directly.

| Symptom | Cause and fix |
|---|---|
| Nothing typed, no error | The focused window is running as administrator. Windows will not let an unelevated process send it input, and running dictate elevated breaks every *other* application instead. |
| Text lands in the wrong application | The wrong window had focus when you pressed the key. Press Ctrl+V — every utterance is on the clipboard too. |
| Text goes to the clipboard every time | You are switching windows before the text is ready. That is the safety net working, not a fault. |
| The first dictation after a pause produces nothing, or only its last words | Look at `mic.firstBuffer` — it says how long the microphone took to deliver anything. A large value means the interface was slow to spin its capture stream up; set `"KeepMicrophoneOpen": true`, at the cost of the microphone indicator staying lit. |
| A held key produces several short utterances instead of one | Every `recording.stop` will read `reason=Reconciled`. dictate is deciding the key was released while you are still holding it — check that `SuppressHotkey` matches your hotkey, and report it with the log. |
| The keyboard stops responding, or every key acts as a shortcut | A modifier is stuck down at the system level. Tap the hotkey once to clear it. |
| Text is rough, fillers left in | Cleanup failed and you got the raw transcript — a notification says so. Usually a missing or expired Anthropic key. |
| A German sentence comes back in English | A bug, not a setting. Cleanup is explicitly instructed never to translate. Please report it. |
| A term is consistently mis-spelled | Add it to `Vocabulary` in the config. |
| The hotkey does nothing | Check the tray icon is there. If it is, restart dictate — another application may have taken the low-level hook. |
| Every utterance fails with a 4xx | Key wrong or expired: tray menu → **Re-enter API keys…**, then restart. |
| Every utterance fails mentioning a model | ElevenLabs changed the Scribe model identifier; set `ScribeModelId` in the config. |

### Rotating a leaked API key

Revoke it at the provider first, then tray menu → **Re-enter API keys…** and
restart dictate. Keys live in Windows Credential Manager under `dictate:*`, never
in a file, so there is nothing else to clean up.
