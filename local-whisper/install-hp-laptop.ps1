<# Optional CPU profile; this calls the general Windows installer. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$DictateExe,
    [string]$PythonExe,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\dictate'),
    [switch]$StartWithWindows,
    [switch]$EnableDiagnosticLog
)
$arguments = @{DictateExe=$DictateExe;Device='cpu';Model='small';Hotkey='CapsLock';InstallDir=$InstallDir;StartWithWindows=$StartWithWindows;EnableDiagnosticLog=$EnableDiagnosticLog}
if ($PythonExe) { $arguments.PythonExe = $PythonExe }
& (Join-Path $PSScriptRoot 'install-windows.ps1') @arguments
