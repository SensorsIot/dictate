<#
.SYNOPSIS
Installs Dictate and local Whisper for the current Windows user.
.EXAMPLE
.\install-windows.ps1 -DictateExe C:\Downloads\dictate.exe -Device cpu -Model small -StartWithWindows
.EXAMPLE
.\install-windows.ps1 -DictateExe C:\Downloads\dictate.exe -Device cuda -Model turbo -StartWithWindows
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$DictateExe,
    [ValidateSet('cpu','cuda')][string]$Device = 'cpu',
    [ValidateSet('tiny','base','small','medium','large-v1','large-v2','large-v3','turbo')][string]$Model = 'small',
    [ValidateSet('Keep','CapsLock','RightWindows','RightCtrl')][string]$Hotkey = 'Keep',
    [ValidateRange(1,65535)][int]$Port = 8178,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\dictate'),
    [string]$PythonExe,
    [switch]$StartWithWindows,
    [switch]$EnableDiagnosticLog
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this installer in a normal PowerShell window, not as administrator.'
}
$sourceExe = (Resolve-Path -LiteralPath $DictateExe).Path
if (-not $PythonExe) {
    if (Get-Command py -ErrorAction SilentlyContinue) {
        $PythonExe = & py -3.12 -c 'import sys; print(sys.executable)'
        if ($LASTEXITCODE -ne 0) { throw 'Install 64-bit Python 3.12, or pass -PythonExe.' }
    } else {
        $PythonExe = (Get-Command python -ErrorAction Stop).Source
    }
}
& $PythonExe -c 'import sys,struct; assert sys.version_info >= (3,12) and struct.calcsize(chr(80)) == 8, str(sys.version)'
if ($LASTEXITCODE -ne 0) { throw '64-bit Python 3.12 or newer is required.' }
$base = [IO.Path]::GetFullPath($InstallDir)
$service = Join-Path $base 'local-whisper'
$runtimePython = Join-Path $base 'whisper-runtime\Scripts\python.exe'
$targetExe = Join-Path $base 'dictate.exe'
$configPath = Join-Path $env:APPDATA 'dictate\config.json'
$logs = Join-Path $env:LOCALAPPDATA 'dictate'
$backup = Join-Path $base ('backup-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$taskName = 'Dictate Local Whisper'
New-Item -ItemType Directory -Path $base,$service,$logs,$backup -Force | Out-Null
foreach ($file in @($configPath,$targetExe,(Join-Path $service 'windows-service.json'))) {
    if (Test-Path -LiteralPath $file) { Copy-Item -LiteralPath $file -Destination $backup }
}
$oldRun = (Get-ItemProperty $runKey -Name dictate -ErrorAction SilentlyContinue).dictate
Set-Content -LiteralPath (Join-Path $backup 'previous-autostart.txt') -Value ([string]$oldRun)
$oldTask = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($oldTask) {
    if (-not @($oldTask.Actions | Where-Object { $_.Arguments -and $_.Arguments.IndexOf($base, [StringComparison]::OrdinalIgnoreCase) -ge 0 }).Count) { throw 'An unrelated task already uses the name Dictate Local Whisper.' }
    Export-ScheduledTask -TaskName $taskName | Set-Content -LiteralPath (Join-Path $backup 'whisper-task.xml')
    try { Register-ScheduledTask -TaskName $taskName -InputObject $oldTask -Force | Out-Null }
    catch { throw 'The existing Whisper task belongs to another/elevated installation. Remove that old task with its owning account first; the running app/configuration have not been changed.' }
}
$oldAppTask = Get-ScheduledTask -TaskName 'Dictate Interactive Launch' -ErrorAction SilentlyContinue
if ($oldAppTask) {
    if (-not @($oldAppTask.Actions | Where-Object { $_.Execute -eq $targetExe }).Count) { throw 'An unrelated task already uses the name Dictate Interactive Launch.' }
    Export-ScheduledTask -TaskName 'Dictate Interactive Launch' | Set-Content -LiteralPath (Join-Path $backup 'dictate-task.xml')
    try { Register-ScheduledTask -TaskName 'Dictate Interactive Launch' -InputObject $oldAppTask -Force | Out-Null }
    catch { throw 'The existing Dictate launch task cannot be updated by this user. Remove it with its owning account first; the running app/configuration have not been changed.' }
}
Write-Host "Backup: $backup"
if (-not (Test-Path -LiteralPath $runtimePython)) {
    & $PythonExe -m venv (Join-Path $base 'whisper-runtime')
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the Python environment.' }
}
$requirements = Join-Path $PSScriptRoot 'requirements-windows.txt'
& $runtimePython -m pip install -r $requirements
if ($LASTEXITCODE -ne 0) { throw 'Whisper installation failed. Existing Dictate has not been stopped.' }
if ($Device -eq 'cuda') {
    & $runtimePython -m pip install -r (Join-Path $PSScriptRoot 'requirements-gpu.txt')
    if ($LASTEXITCODE -ne 0) { throw 'CUDA runtime installation failed.' }
}
& $runtimePython -m unittest discover -s $PSScriptRoot -p 'test_*.py'
if ($LASTEXITCODE -ne 0) { throw 'Local service/installer checks failed.' }
# Validate the model and prepare a staged config before stopping the working app.
$stagedConfig = Join-Path $backup 'prepared-config.json'
if (Test-Path -LiteralPath $configPath) { Copy-Item -LiteralPath $configPath -Destination $stagedConfig }
& $runtimePython (Join-Path $PSScriptRoot 'configure_windows.py') --config $stagedConfig --model $Model --device $Device --port $Port --hotkey $Hotkey
if ($LASTEXITCODE -ne 0) { throw 'Model/configuration validation failed; existing configuration is unchanged.' }
if ($EnableDiagnosticLog) {
    $prepared = [IO.File]::ReadAllText($stagedConfig) | ConvertFrom-Json
    $prepared | Add-Member -MemberType NoteProperty -Name EnableDiagnosticLog -Value $true -Force
    [IO.File]::WriteAllText($stagedConfig, ($prepared | ConvertTo-Json -Depth 20), (New-Object Text.UTF8Encoding($false)))
}
& $runtimePython -m pip freeze | Set-Content -LiteralPath (Join-Path $backup 'installed-python-packages.txt')
$ownedServers = @(Get-CimInstance Win32_Process -Filter "Name = 'python.exe'" | Where-Object { $_.CommandLine -and $_.CommandLine.IndexOf((Join-Path $service 'server.py'), [StringComparison]::OrdinalIgnoreCase) -ge 0 })
$listeners = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
foreach ($listener in $listeners) {
    if ($listener.OwningProcess -notin $ownedServers.ProcessId) { throw "Port $Port belongs to another installation. Stop it or choose -Port." }
}
if ($oldTask) { Stop-ScheduledTask -TaskName $taskName }
foreach ($server in $ownedServers) { Stop-Process -Id $server.ProcessId -ErrorAction SilentlyContinue }
# The user's previous installed path is also allowed; save it for rollback.
$dictateProcesses = @(Get-CimInstance Win32_Process -Filter "Name = 'dictate.exe'" | Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($base + '\', [StringComparison]::OrdinalIgnoreCase) })
foreach ($app in $dictateProcesses) {
    Copy-Item -LiteralPath $app.ExecutablePath -Destination (Join-Path $backup 'previous-running.exe') -Force
    Set-Content -LiteralPath (Join-Path $backup 'previous-executable.txt') -Value $app.ExecutablePath
    Stop-Process -Id $app.ProcessId
    Wait-Process -Id $app.ProcessId -Timeout 10 -ErrorAction SilentlyContinue
}
$files = @('server.py','configure_windows.py','start-windows.ps1','install-windows.ps1','install-hp-laptop.ps1','requirements.txt','requirements-windows.txt','requirements-gpu.txt','test_server.py','test_install_windows.py')
foreach ($name in $files) {
    $source = Join-Path $PSScriptRoot $name
    $destination = Join-Path $service $name
    if ([IO.Path]::GetFullPath($source) -ne [IO.Path]::GetFullPath($destination)) { Copy-Item -LiteralPath $source -Destination $destination -Force }
}
if ($sourceExe -ne $targetExe) { Copy-Item -LiteralPath $sourceExe -Destination $targetExe -Force }
New-Item -ItemType Directory -Path (Split-Path -Parent $configPath) -Force | Out-Null
Copy-Item -LiteralPath $stagedConfig -Destination $configPath -Force
$serviceSettings = @{Model=$Model;Device=$Device;Port=$Port;ComputeType=$(if ($Device -eq 'cuda') {'int8_float16'} else {'int8'})}
[IO.File]::WriteAllText((Join-Path $service 'windows-service.json'), ($serviceSettings | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
$user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$taskPrincipal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$taskSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
$launcher = Join-Path $service 'start-windows.ps1'
$action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + $launcher + '"') -WorkingDirectory $service
$taskArguments = @{TaskName=$taskName;Action=$action;Principal=$taskPrincipal;Settings=$taskSettings;Description='Local Whisper for Dictate, loopback only, unelevated.';Force=$true}
# Preserve existing autostart; adding it on a new PC requires the explicit switch.
if ($StartWithWindows -or ($oldTask -and $oldTask.Triggers)) { $taskArguments.Trigger = New-ScheduledTaskTrigger -AtLogOn -User $user }
Register-ScheduledTask @taskArguments | Out-Null
Start-ScheduledTask -TaskName $taskName
$ready = $false
$deadline = (Get-Date).AddSeconds(90)
while ((Get-Date) -lt $deadline) {
    try {
        $health = Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/health" -f $Port) -TimeoutSec 2
        if ($health.status -eq 'ready' -and $health.model -eq $Model) { $ready=$true; break }
    } catch [System.Net.WebException] {}
    Start-Sleep -Seconds 1
}
if (-not $ready) { throw "Whisper did not become ready. See $logs\whisper-service.err.log. Backup: $backup" }
if ($StartWithWindows -or $oldRun) {
    if (-not (Test-Path $runKey)) { New-Item -Path $runKey | Out-Null }
    Set-ItemProperty $runKey -Name dictate -Value ('"' + $targetExe + '"')
}
# Task Scheduler launches in the interactive user session even when installed via SSH.
$appAction = New-ScheduledTaskAction -Execute $targetExe -WorkingDirectory $base
Register-ScheduledTask -TaskName 'Dictate Interactive Launch' -Action $appAction -Principal $taskPrincipal -Settings $taskSettings -Force | Out-Null
Start-ScheduledTask -TaskName 'Dictate Interactive Launch'
$report = [ordered]@{InstalledAt=(Get-Date).ToString('o');Executable=$targetExe;SHA256=(Get-FileHash -LiteralPath $targetExe).Hash;Model=$Model;Device=$Device;ComputeType=$serviceSettings.ComputeType;Port=$Port;Language='Auto';Cleanup='None';Hotkey=$Hotkey;Backup=$backup;Logs=$logs}
[IO.File]::WriteAllText((Join-Path $logs 'local-installation.json'), ($report | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
Write-Host "Installed: $targetExe"
Write-Host "Whisper $Model is ready on $Device. Dictate launch requested."
Write-Host "Logs: $logs; backup: $backup"
Write-Host 'Test the hotkey and microphone in a text editor before relying on dictation.'
