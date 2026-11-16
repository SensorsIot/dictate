# Invoked by the installer or the per-user logon task.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$settings = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'windows-service.json')) | ConvertFrom-Json
$base = Split-Path -Parent $PSScriptRoot
$python = Join-Path $base 'whisper-runtime\Scripts\python.exe'
$server = Join-Path $PSScriptRoot 'server.py'
$logs = Join-Path $env:LOCALAPPDATA 'dictate'
New-Item -ItemType Directory -Path $logs -Force | Out-Null
$stderr = Join-Path $logs 'whisper-service.err.log'
try {
    # A second launcher must not start a duplicate model process.
    $health = $null
    try { $health = Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/health" -f $settings.Port) -TimeoutSec 2 } catch [System.Net.WebException] {}
    if ($health) {
        if ($health.status -eq 'ready' -and $health.model -eq $settings.Model) { exit 0 }
        throw 'The configured Whisper port is occupied by another service or model.'
    }
    # PowerShell 5.1 Start-Process joins ArgumentList; quote paths explicitly.
    $arguments = '"{0}" --device {1} --compute-type {2} --offline' -f $server, $settings.Device, $settings.ComputeType
    $process = Start-Process -FilePath $python -ArgumentList $arguments -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -RedirectStandardOutput (Join-Path $logs 'whisper-service.out.log') -RedirectStandardError $stderr -Wait -PassThru
    exit $process.ExitCode
} catch {
    Add-Content -LiteralPath $stderr -Value ("{0:u} Startup failed: {1}" -f (Get-Date), $_.Exception.Message)
    exit 1
}
