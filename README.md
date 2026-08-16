# dictate

Push-to-talk dictation for Windows. Hold a key, speak, release — the cleaned-up text is
typed into whatever window has focus.

Speech goes to **ElevenLabs Scribe**, the raw transcript is tidied by **Claude Haiku 4.5**
(fillers removed, punctuation fixed, your own vocabulary spelled correctly), and the result
is injected with `SendInput`. German and English, auto-detected.

Nothing is written to disk: no audio files, no transcript history.

> Status: early. See [`docs/DESIGN.md`](docs/DESIGN.md) for the decisions behind it.

## Install

Download `dictate.exe` from the [latest release](https://github.com/SensorsIot/dictate/releases)
— it is self-contained, so no .NET runtime is needed. Then:

```
dictate.exe --auth
```

which stores your ElevenLabs and Anthropic API keys in Windows Credential Manager
(DPAPI-encrypted, per-user). Run `dictate.exe` to start; it lives in the tray.

## Use

Hold **Right Ctrl**, speak, release. The text appears where your cursor is.

If you alt-tab while it is transcribing, the text goes to the clipboard instead of being
typed into the wrong window.

## Build

The repo builds in GitHub Actions — see [`.github/workflows/`](.github/workflows/). There is
no .NET SDK in the devcontainer by design; `dotnet` commands run in CI, not locally.
