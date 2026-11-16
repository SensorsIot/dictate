# dictate — User Manual

> Everything below describes actual behaviour on a real machine, not intended
> behaviour.

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
   one file — there is nothing else to install.
2. Put it somewhere permanent, e.g. `%LOCALAPPDATA%\dictate\dictate.exe`.
   Windows SmartScreen will warn about an unsigned executable the first time;
   *More info → Run anyway*.
3. Store your API keys:

   ```
   dictate.exe --auth
   ```

   You will be asked for an **ElevenLabs** key (speech-to-text) and an
   **Anthropic** key (cleanup) — or an **Infomaniak** key if you chose
   Infomaniak for either (see *Providers* below). They are written to Windows Credential Manager,
   encrypted under your Windows account. They are never written to a file.

4. To start dictate at login, right-click its tray icon and tick **Start with
   Windows**. It is off until you do. Untick it to stop.

**Do not run dictate as administrator.** It will appear to work and then
silently fail to type into ordinary applications — Windows does not let an
elevated process send keystrokes to unelevated windows.

### On a Mac

1. Download `dictate-macos.zip` from the same release, unzip it, and move
   `dictate.app` to **Applications**. It runs on Apple Silicon and Intel.
2. Open it. It asks for the **ElevenLabs** and **Anthropic** keys (or **Infomaniak**, see *Providers*) and stores
   them in your login Keychain, never in a file. **Re-enter API keys…** in its
   menu, or `dictate.app/Contents/MacOS/dictate --auth`, asks again.
3. Allow what macOS asks for, in **System Settings → Privacy & Security**:
   - **Accessibility** — to type the text and read window titles.
   - **Input Monitoring** — to see the hotkey. dictate starts listening as soon
     as it is switched on.
   - **Microphone** — asked at the first dictation.
4. dictate is the dot in the menu bar: grey idle, red recording, blue
   transcribing. It has no Dock icon and no on-screen indicator.
5. To start it at login, add it under **System Settings → General → Login
   Items**.

The hotkey is **Right Command** — the right Windows key on a Windows keyboard.
`HotkeyVirtualKey` uses the Windows key codes on both systems, so one
config.json means the same key on each; on a Mac it lives in
`~/Library/Application Support/dictate/`, and the log in
`~/Library/Logs/dictate/`. The Mac supports the modifier keys and F1–F20 as
hotkeys; `SuppressHotkey` has no effect there, because the Mac hotkey is never
swallowed.

On a Windows keyboard without a right Windows key, use **Right Ctrl**
(`"HotkeyVirtualKey": 163`), or Pause, which the Mac reads as F15
(`"HotkeyVirtualKey": 126`). On a Mac, Control on its own does nothing, and
Control+scroll zooms only if you have switched on zoom in the Accessibility settings. Avoid
Right Alt: it is the right Option key, which types `@`, `#` and `|` on a Swiss
layout. Caps Lock does not work as a Mac hotkey: macOS reports only when it
toggles, not how long it is held.

## 3. Everyday use

Hold **Right Windows**, speak, release.

| What you see | What is happening |
|---|---|
| Overlay says *recording* | Listening |
| Overlay says *transcribing* | Uploading and cleaning up |
| Overlay clears, text appears | Done |
| Notification | Something went wrong. It says what happened to your words — not transcribed, typed as spoken, or on the clipboard — and what to do: wait out an outage, top up a provider, or re-enter a key |

You don't have to wait for the text before the next sentence: press again
while it is still *transcribing* and it records at once. The sentences are
typed in the order you spoke them, and nothing is typed while you are holding the key.

Two rules worth knowing:

- **Don't switch windows while it is thinking.** If the focused window changes
  between releasing the key and the text being ready, dictate puts the text on
  the clipboard and tells you, rather than typing it into whatever is now in
  front. Press Ctrl+V where you actually wanted it.
- **In a terminal you always get a single line.** Line breaks are stripped,
  because a newline typed into a shell is the Enter key and would run the
  command.

The hotkey is swallowed, so the right Windows key does not open the Start menu
or acts as a modifier. The left Windows key is untouched.

## 4. Configuration

`%APPDATA%\dictate\config.json`. Missing file means defaults. Edit it and restart
dictate.

The settings you are most likely to want:

| Setting | Default | What it does |
|---|---|---|
| `Language` | `Auto` | `German` or `English` pins the language when auto-detection keeps guessing wrong on short phrases. `Auto` lets the provider choose; the diagnostic log shows what it detected (`lang=`) |
| `SwissSpelling` | `true` | Swiss Standard German spelling: `ss`, never `ß`. Applies even when cleanup fails |
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
| `InputDevice` | `null` | Part of the name of the microphone to use, e.g. `"BRIO"` for "Microphone (Logitech BRIO)". Without it, or if nothing matches, dictate uses the Windows default input. Also selectable from the tray menu under **Microphone**, which saves the choice here |
| `KeepMicrophoneOpen` | `false` | Hold the capture device open. Removes device-open latency, but lights the Windows microphone indicator permanently. When off, the microphone still stays open for 3 s after each dictation, so a quick follow-up press captures at once; your text is not delayed by this |
| `ExtraConsoleProcesses` | `[]` | Extra terminal executables that must never receive newlines |

### Providers

By default ElevenLabs transcribes and Anthropic (Claude Haiku) cleans up.
Either can be switched to Infomaniak, which processes everything in
Switzerland. One Infomaniak token covers both.

| Setting | Default | What it does |
|---|---|---|
| `TranscriptionProvider` | `ElevenLabs` | `Infomaniak` for Whisper hosted in Switzerland, or `LocalWhisper` for speech-to-text on this computer (chapter 7) |
| `CleanupProvider` | `Anthropic` | `Infomaniak` for cleanup hosted in Switzerland, or `None` to disable cleanup even when keys are stored |
| `TranscriptionModel` | provider's default | `scribe_v2` (from `ScribeModelId`) for ElevenLabs, `whisper` for Infomaniak |
| `CleanupModel` | provider's default | `claude-haiku-4-5` for Anthropic, `google/gemma-4-31B-it` for Infomaniak. Other Infomaniak models tested worse: they translated or answered instead of cleaning |
| `InfomaniakProductId` | found automatically | Only needed if your Infomaniak account has more than one AI Services product |

Each provider's key is stored separately. After switching, restart dictate and
use **Re-enter API keys…** once to enter the new provider's key. The Infomaniak
token is created in the Infomaniak Manager under AI Services; it is shown only
once.

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
| **Caps Lock** | **`20`** | The choice for laptops without a right Windows key: on the left and easy to hold. With `"ShortPressPassesThrough": true` a quick tap still toggles Caps Lock and only a hold dictates |
| Scroll Lock | `145` | Genuinely unused on modern machines, but absent from many compact keyboards |

**A Windows key requires `"SuppressHotkey": true` in the same edit.** Pressing
and releasing either Windows key with nothing in between opens the Start menu, so
without suppression you get the Start menu on every dictation. Suppression stops
the key reaching the shell — at the price of every Win+*key* shortcut made with
that hand. On the right Windows key that price is usually zero.

**`ShortPressPassesThrough`** (default `false`) gives a hotkey back its own
function on a quick tap: a press shorter than `MinimumHoldMs` (300 ms) is passed
on to Windows as a normal keystroke after you let go. Use it with Caps Lock.
With a Windows key it would open the Start menu on every tap. Windows only.

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
| "Administrator window" notification | You pressed the hotkey in a window running as administrator. Windows does not show dictate keys pressed there, so the key reached Windows and opened the Start menu; dictate closes it again. Dictate in an ordinary window and paste. The other Windows key is closed the same way in administrator windows — use the Start button instead. |
| Text lands in the wrong application | The wrong window had focus when you pressed the key. Press Ctrl+V — every utterance is on the clipboard too. |
| Text goes to the clipboard every time | You are switching windows before the text is ready. That is the safety net working, not a fault. |
| The first dictation after a pause produces nothing, or only its last words | Look at `mic.firstBuffer` — it says how long the microphone took to deliver anything. A large value means the interface was slow to spin its capture stream up; set `"KeepMicrophoneOpen": true`, at the cost of the microphone indicator staying lit. |
| A held key produces several short utterances instead of one | Every `recording.stop` will read `reason=Reconciled`. dictate is deciding the key was released while you are still holding it — check that `SuppressHotkey` matches your hotkey, and report it with the log. |
| The keyboard stops responding, or every key acts as a shortcut | A modifier is stuck down at the system level. Tap the hotkey once to clear it. |
| Text is rough, fillers left in | Cleanup failed and you got the raw transcript — a notification says so. Usually a missing or expired cleanup key (Anthropic or Infomaniak). |
| A German sentence comes back in English | A bug, not a setting. Cleanup is explicitly instructed never to translate. Please report it. |
| A term is consistently mis-spelled | Add it to `Vocabulary` in the config. |
| The hotkey does nothing | Check the tray icon is there. If it is, restart dictate — another application may have taken the low-level hook. |
| "*Provider* is overloaded" or "is having problems" | An outage at that provider, not on your side. Wait a few minutes. The diagnostic log has the provider's request id if you want to report it. |
| "*Provider* credit used up" | Top up at that provider — the notification says where. For ElevenLabs, the monthly reset also restores it. |
| "*Provider* rejected the API key" | Key wrong, revoked or expired: tray menu → **Re-enter API keys…**, then restart. |
| "*Provider* rejected the request" | Usually a model the provider no longer offers; the notification quotes the provider. Set `TranscriptionModel` or `CleanupModel` in the config. |

### Rotating a leaked API key

Revoke it at the provider first, then tray menu → **Re-enter API keys…** and
restart dictate. Keys live in Windows Credential Manager under `dictate:*`, never
in a file, so there is nothing else to clean up.

## 7. Local Whisper speech-to-text

Local Whisper runs OpenAI's speech recognition models through
[faster-whisper](https://github.com/SYSTRAN/faster-whisper). Dictate sends audio
only to the local service at `127.0.0.1`. Audio and transcripts stay in memory;
downloaded model weights remain cached on disk. There is no cloud fallback.

### Select the model

Edit `%APPDATA%\dictate\config.json` (or the Mac configuration file):

```json
{
  "TranscriptionProvider": "LocalWhisper",
  "TranscriptionModel": "turbo",
  "CleanupProvider": "None",
  "LocalWhisperPort": 8178,
  "LocalWhisperTimeoutSeconds": 60
}
```

`TranscriptionModel` is the speech-to-text model selection. Supported values:

| Models | Use |
|---|---|
| `tiny`, `base` | Smaller models for limited hardware |
| `small`, `medium` | Larger alternatives to compare for your language and hardware |
| `large-v1`, `large-v2`, `large-v3` | Full large models, requiring more memory and processing |
| `turbo` | Optimized large-v3 model; default for local speech-to-text |
| `tiny.en`, `base.en`, `small.en`, `medium.en` | English-only variants; use multilingual models for German |

`turbo` is a reasonable starting point for an RTX 2070 SUPER with 8 GB VRAM.
The service uses `int8_float16` on NVIDIA GPUs to reduce memory consumption.
Speed and available memory depend on other running applications; try `small`
if memory or latency is a problem. GPU performance has to be measured on the
target PC. `Language` and `Vocabulary` also apply to local transcription.

`CleanupProvider: None` makes dictation fully local and requires no API keys.
Selecting Anthropic or Infomaniak cleanup sends the recognized text to that
provider when its key is stored. Swiss spelling and safe text delivery apply
with cleanup disabled too.

### Install the optional runtime

For a Windows PC or laptop, follow the
[general scripted installation](Windows-Installation.md). It includes CPU/GPU
commands, model changes, login startup, recovery and an optional laptop profile.
The commands below describe manual runtime management instead.

The normal cloud mode still needs only the Dictate executable. Local mode also
needs the repository's `local-whisper` directory and Python 3.9 or newer.
From the repository folder, in PowerShell:

```powershell
py -3 -m venv .venv-whisper
.\.venv-whisper\Scripts\python.exe -m pip install -r local-whisper\requirements.txt
```

For NVIDIA GPU execution, install CUDA 12 cuBLAS and cuDNN 9 libraries and make
their DLL directories available on `PATH`, following the
[faster-whisper GPU requirements](https://github.com/SYSTRAN/faster-whisper#gpu).
On Windows, install the optional cuBLAS pip runtime into the same environment:

```powershell
.\.venv-whisper\Scripts\python.exe -m pip install -r local-whisper\requirements-gpu.txt
```

The service discovers NVIDIA pip DLLs automatically without changing the
system `PATH`. CTranslate2 4.9 bundles cuDNN; versions without it still need
cuDNN 9 installed separately.
The NVIDIA display driver alone does not supply these runtime libraries.
CPU execution is available without CUDA. Macs use the CPU backend.

### Start and change models

Start the service with the same configuration Dictate reads:

```powershell
.\.venv-whisper\Scripts\python.exe local-whisper\server.py --device cuda
```

The first start downloads the selected model. Wait for `Ready`, keep the service
running, then start the newly built Dictate executable. No Whisper model download
or installation happens automatically inside the tray application.

Use `--device cpu` on a PC without a supported NVIDIA GPU, or omit `--device`
to detect NVIDIA hardware automatically. Automatic detection does not hide CUDA
installation failures by switching to CPU. On macOS/Linux use
`.venv-whisper/bin/python` in place of the Windows Python path.

To use another configuration file, append `--config "path/to/config.json"`.
Add `--offline` after the first download to require cached model files only.
For example, to switch to `small`, edit `TranscriptionModel` to `small`, stop
the service with Ctrl+C, start it again, wait for `Ready`, and restart Dictate.
The service keeps one model loaded and rejects mismatched requests.

If Dictate cannot reach the service, start it and check that `LocalWhisperPort`
matches. On a timeout, try a smaller model or raise
`LocalWhisperTimeoutSeconds`. On startup or GPU errors, check the service's
terminal and verify the CUDA libraries and available GPU memory. Quitting
Dictate leaves the independently started service running; stop the service to
release its model memory.
