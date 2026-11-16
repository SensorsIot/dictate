# Install Dictate with local Whisper on Windows

This procedure applies to Windows PCs and laptops. It installs for the current
user, without administrator rights. Speech recognition runs on that computer;
no cloud key is needed. The optional laptop profile is a shortcut to the same
installer, not a separate program.

## Requirements

- Windows 10/11, 64-bit, and Windows PowerShell 5.1.
- 64-bit Python 3.12, with the Python launcher (`py`). If using another Python
  installation, supply its full path with `-PythonExe`.
- `dictate.exe` and the complete `local-whisper` folder from the same version.
  Download the executable from the project's release/build artifacts and obtain
  the folder from the matching source archive or checkout. No Rust toolchain
  is needed on the destination computer.
- Internet access for Python packages and the model's first download. Subsequent
  service starts use cached model files and work offline.
- Enough free disk space for the Python environment and models. Larger models
  need several GB. CPU mode needs no NVIDIA software. GPU mode requires a
  compatible NVIDIA GPU and driver, plus enough free VRAM.

Copy these files to the destination PC. Close its running Dictate instance
before an upgrade, particularly if it lives outside the standard installation
folder. Open a **normal PowerShell window**, not "Run as administrator", in
the directory containing `local-whisper`. Back up important hand-edited settings
before installing; the installer also creates a local backup.

## CPU installation

Start with multilingual `small` and INT8 on a PC without a suitable NVIDIA GPU:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\local-whisper\install-windows.ps1 `
  -DictateExe "C:\Downloads\dictate.exe" -Device cpu -Model small -StartWithWindows
```

Replace the executable path with the file you downloaded. `-StartWithWindows`
enables startup of both Dictate and the local service at this user's login.
Without the switch, an existing startup setting is preserved; a new installation
starts now but does not add automatic login startup. The script refuses an
elevated window because Dictate must run as the ordinary desktop user.

If Python cannot be found:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\local-whisper\install-windows.ps1 `
  -DictateExe "C:\Downloads\dictate.exe" -Device cpu -Model small `
  -PythonExe "C:\Path\To\Python312\python.exe" -StartWithWindows
```

## NVIDIA GPU installation

Choose GPU execution explicitly. The installer adds the required cuBLAS runtime
inside its Python environment; the tested CTranslate2 version supplies cuDNN.
It does not install a graphics driver or change the system `PATH`.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\local-whisper\install-windows.ps1 `
  -DictateExe "C:\Downloads\dictate.exe" -Device cuda -Model turbo -StartWithWindows
```

GPU mode uses `int8_float16`. An unsuitable GPU or missing runtime produces an
error, rather than silently selecting the CPU. Try CPU mode if GPU memory is
limited or shared with demanding applications. Memory consumption and speed
must be measured on the destination PC.

## Model and keyboard options

The general installer accepts `tiny`, `base`, `small`, `medium`, `large-v1`,
`large-v2`, `large-v3`, and `turbo`. All are multilingual. It sets `Language`
to `Auto` and disables cloud cleanup with `CleanupProvider: None`.

| Option | Behavior |
|---|---|
| `-Device cpu` | CPU with INT8; default |
| `-Device cuda` | NVIDIA GPU with INT8/FP16 |
| `-Model small` | Default; a starting point for CPU dictation |
| `-Model turbo` | A starting point for GPU dictation |
| `-Model large-v3` | Full model to compare for accuracy; slower |
| `-Hotkey Keep` | Preserve existing keyboard settings; default. A new config uses Dictate's right Windows key default |
| `-Hotkey CapsLock` | Caps Lock: a quick tap toggles it; hold to dictate |
| `-Hotkey RightWindows` | Right Windows, suppressed during dictation |
| `-Hotkey RightCtrl` | Right Ctrl, suppressed during dictation |
| `-Port 8178` | IPv4 loopback service port; default |
| `-InstallDir "C:\Path\dictate"` | Override the per-user installation directory |
| `-EnableDiagnosticLog` | Opt into metadata-only logging; otherwise preserve the existing preference |

The microphone, vocabulary and other unrelated settings are retained. The
original JSON/JSON5 configuration is backed up; the working file is rewritten
as JSON, so comments are not retained in that working copy.

For a laptop without a right Windows key, add `-Hotkey CapsLock` to the general
command. The optional **HP laptop profile** supplies CPU, `small`, and Caps Lock:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\local-whisper\install-hp-laptop.ps1 `
  -DictateExe "C:\Downloads\dictate.exe" -StartWithWindows
```

This profile explicitly replaces any previous hotkey with Caps Lock. It does
not assume that every HP laptop has the same hardware and does not probe the
manufacturer. Use the general installer for a different model, GPU or hotkey.

## What the installer does

1. Saves the previous executable, configuration, startup setting and existing
   service task, where present, to a timestamped `backup-*` directory.
2. Creates a dedicated Python environment and installs the tested Windows
   dependencies. GPU mode installs its additional runtime.
3. Runs the service/configuration checks, downloads the selected model and
   verifies that it loads on the requested device before replacing the config.
4. Copies the executable and service files, preserves microphone/keyboard
   settings unless overridden, and selects fully local automatic transcription.
5. Starts the hidden local service as the logged-on user, checks `/health`, and
   launches Dictate unelevated in that user's desktop session. This also works
   when the script is invoked through an existing SSH connection to that user.
6. Saves an installation report, executable hash and resolved Python package
   versions. Login startup is enabled only when requested or already configured.

Default installed paths:

```text
%LOCALAPPDATA%\Programs\dictate\dictate.exe
%LOCALAPPDATA%\Programs\dictate\local-whisper\
%LOCALAPPDATA%\Programs\dictate\whisper-runtime\
%APPDATA%\dictate\config.json
%LOCALAPPDATA%\dictate\local-installation.json
%LOCALAPPDATA%\dictate\whisper-service.out.log
%LOCALAPPDATA%\dictate\whisper-service.err.log
```

The task **Dictate Local Whisper** supervises the local service and can start
it at login. **Dictate Interactive Launch** provides an on-demand desktop
launch. Dictate's usual per-user `Run` entry starts its tray app at login.
The service accepts connections on `127.0.0.1` only, with no firewall opening.
Audio and dictated text stay in memory; diagnostic logging records metadata.

## Check the installation

Wait for the installer to report that Whisper is ready. Click inside a text
editor, hold the configured hotkey, speak, then release it. Test German and
English separately. A correct model load and health response do not prove that
the selected microphone or hotkey works on that computer.

```powershell
Invoke-RestMethod http://127.0.0.1:8178/health
Get-ScheduledTask -TaskName "Dictate Local Whisper"
```

If logging was enabled, inspect `%LOCALAPPDATA%\dictate\dictate.log` for
`recording.start`, `recording.stop`, transcription outcome and delivery.
Check the two service logs for runtime/startup errors. For a different port,
adjust the health URL accordingly.

## Change models or update the installation

Run the general installer again with the desired device/model and the new
executable. It reuses the Python environment and cached models, creates another
backup, updates the service settings, and restarts both components.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\local-whisper\install-windows.ps1 `
  -DictateExe "C:\Downloads\dictate.exe" -Device cuda -Model large-v3
```

Only the active model occupies runtime memory; cached model files remain on
disk. Editing `TranscriptionModel` alone is insufficient for a managed
installation: rerun the installer so its launcher settings and both running
processes agree. A new model needs a one-time download; normal service startup
uses `--offline`.

## Recover or remove

The installer reports its backup directory. It does not automatically undo a
failed installation. If model validation fails, the old configuration and
running Dictate remain unchanged. For a later startup failure, inspect the
service error log and the backup before retrying.

If an earlier administrator-run deployment owns the Dictate scheduled tasks,
the normal-user installer may be unable to update them. It checks this before
stopping the working app. Have the account that created those tasks export and
remove only **Dictate Local Whisper** and **Dictate Interactive Launch**, then
rerun the installer in the normal user window. Do not run the installer itself
as administrator to work around that ownership problem.

To restore a previous setup, exit Dictate, stop **Dictate Local Whisper**, and
restore the backed-up `config.json` and executable. Restore the previous
per-user `Run` command from `previous-autostart.txt`; the previous scheduled
task is saved as `whisper-task.xml`, if one existed. Do not restore a service
task without its matching service files/configuration.

To remove this local installation, exit Dictate, stop and remove the two
Dictate scheduled tasks, disable Dictate's "Start with Windows", and remove
only the installation directory you selected. Keep the configuration and model
cache if they should be reused. No system CUDA installation needs removal.
