param([switch]$WithoutMl, [switch]$NoPlugin, [switch]$NoStart)
$ErrorActionPreference = "Stop"
$Source = $PSScriptRoot
$Version = (Get-Content (Join-Path $Source "VERSION")).Trim()
if ($Version -notmatch '^[0-9A-Za-z._+-]+$') { throw "Release VERSION is invalid" }
$InstallRoot = if ($env:OPENCHATCUT_INSTALL_ROOT) { $env:OPENCHATCUT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "OpenChatCut" }
$Destination = Join-Path $InstallRoot "versions\$Version"
$HomeDir = if ($env:OPENCHATCUT_HOME) { $env:OPENCHATCUT_HOME } else { Join-Path $HOME ".openchatcut" }
if (-not (Get-Command python -ErrorAction SilentlyContinue)) { throw "Python 3.11+ is required" }
python (Join-Path $Source "scripts\release\verify-release-bundle.py") $Source
if ($LASTEXITCODE -ne 0) { throw "Release verification failed" }
New-Item -ItemType Directory -Force -Path (Split-Path $Destination),$HomeDir | Out-Null
if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
Copy-Item -Recurse -Force $Source $Destination
$Shim = Join-Path $InstallRoot "openchatcut.ps1"
Set-Content (Join-Path $InstallRoot "CURRENT") $Version
Copy-Item (Join-Path $Destination "scripts\release\openchatcut-installed.ps1") $Shim -Force

& (Join-Path $Destination "scripts\install-ffmpeg.ps1")
$Venv = Join-Path $HomeDir "runtime\media-worker"
$Python = Join-Path $Venv "Scripts\python.exe"
if (-not (Test-Path $Python)) { python -m venv $Venv }
& $Python -m pip install --disable-pip-version-check --upgrade pip
$Extras = if ($WithoutMl) { "[render]" } else { "[transcription,diarization,denoise,render]" }
& $Python -m pip install --disable-pip-version-check "$(Join-Path $Destination 'services\media-worker')$Extras"
$ChromeCandidates = @(
  (Join-Path $env:LOCALAPPDATA "Google\Chrome\Application\chrome.exe"),
  (Join-Path $env:PROGRAMFILES "Google\Chrome\Application\chrome.exe"),
  (Join-Path ${env:PROGRAMFILES(X86)} "Google\Chrome\Application\chrome.exe")
)
if ($ChromeCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1) {
  Write-Host "Using the installed Chrome browser for headless rendering"
} else {
  & $Python -m playwright install chromium
}
if (-not $NoPlugin -and (Get-Command codex -ErrorAction SilentlyContinue) -and (Get-Command node -ErrorAction SilentlyContinue)) {
  & (Join-Path $Destination "scripts\install-codex-plugin.ps1")
} elseif (-not $NoPlugin) {
  Write-Warning "Codex plugin skipped; install Codex + Node and run the bundled plugin installer."
}
Write-Host "Installed OpenChatCut $Version at $Destination"
Write-Host "Run: powershell -ExecutionPolicy Bypass -File `"$Shim`" open"
if (-not $NoStart) { & $Shim open }
