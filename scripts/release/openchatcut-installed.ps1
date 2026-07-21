param(
  [ValidateSet("start", "stop", "restart", "status", "open", "logs")]
  [string]$Command = "start"
)
$ErrorActionPreference = "Stop"
$CurrentFile = Join-Path $PSScriptRoot "CURRENT"
if (-not (Test-Path $CurrentFile)) { throw "OpenChatCut has no installed current version" }
$Version = (Get-Content $CurrentFile -Raw).Trim()
if ($Version -notmatch '^[0-9A-Za-z._+-]+$') { throw "OpenChatCut CURRENT contains an invalid version" }
$Launcher = Join-Path $PSScriptRoot "versions\$Version\openchatcut.ps1"
if (-not (Test-Path $Launcher)) { throw "OpenChatCut version $Version is incomplete" }
& $Launcher -Command $Command
exit $LASTEXITCODE
