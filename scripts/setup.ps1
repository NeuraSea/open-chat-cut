param(
  [switch]$WithoutMl,
  [switch]$SkipWeb
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

foreach ($Command in @("cargo", "python", "node", "npm")) {
  if (-not (Get-Command $Command -ErrorAction SilentlyContinue)) {
    throw "Missing required command: $Command"
  }
}
if (-not $SkipWeb -and -not (Get-Command docker -ErrorAction SilentlyContinue)) {
  throw "Missing required command: docker"
}

function Invoke-Native {
  param([string]$Command, [string[]]$Arguments)
  & $Command @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Command failed with exit code $LASTEXITCODE"
  }
}

Write-Host "[0/3] Ensuring FFmpeg/ffprobe are available"
& (Join-Path $PSScriptRoot "install-ffmpeg.ps1")

Write-Host "[1/3] Building the native daemon"
Push-Location $Root
try { Invoke-Native cargo @("build", "--release", "-p", "openchatcut-daemon", "--bin", "openchatcutd") } finally { Pop-Location }
Invoke-Native npm @("install", "--prefix", (Join-Path $Root "packages\mg-runtime"), "--omit=dev", "--ignore-scripts", "--no-package-lock")

Write-Host "[2/3] Preparing the native media worker"
$Python = Join-Path $Root ".venv\Scripts\python.exe"
if (-not (Test-Path $Python)) { python -m venv (Join-Path $Root ".venv") }
Invoke-Native $Python @("-m", "pip", "install", "--disable-pip-version-check", "--upgrade", "pip")
$Worker = Join-Path $Root "services\media-worker"
if ($WithoutMl) { Invoke-Native $Python @("-m", "pip", "install", "--disable-pip-version-check", "-e", "$Worker[render]") }
else { Invoke-Native $Python @("-m", "pip", "install", "--disable-pip-version-check", "-e", "$Worker[transcription,diarization,render]") }
$ChromeCandidates = @(
  (Join-Path $env:LOCALAPPDATA "Google\Chrome\Application\chrome.exe"),
  (Join-Path $env:PROGRAMFILES "Google\Chrome\Application\chrome.exe"),
  (Join-Path ${env:PROGRAMFILES(X86)} "Google\Chrome\Application\chrome.exe")
)
if ($ChromeCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1) {
  Write-Host "Using the installed Chrome browser for headless rendering"
} else {
  Invoke-Native $Python @("-m", "playwright", "install", "chromium")
}

if (-not $SkipWeb) {
  Write-Host "[3/3] Building the Web editor image"
  Push-Location $Root
  try { Invoke-Native docker @("compose", "build", "web") } finally { Pop-Location }
} else { Write-Host "[3/3] Skipping the Web editor image" }

Write-Host "`nOpenChatCut is installed. Next:"
Write-Host "  1. codex login"
Write-Host "  2. .\scripts\openchatcut.ps1 start"
Write-Host "  3. .\scripts\install-codex-plugin.ps1"
