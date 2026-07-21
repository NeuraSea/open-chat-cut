param()

$ErrorActionPreference = "Stop"

function Refresh-ProcessPath {
  $Machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $User = [Environment]::GetEnvironmentVariable("Path", "User")
  $env:Path = @($Machine, $User) -join ";"
}

if ((Get-Command ffmpeg -ErrorAction SilentlyContinue) -and
    (Get-Command ffprobe -ErrorAction SilentlyContinue)) {
  Write-Host "FFmpeg/ffprobe already available"
  exit 0
}

if (Get-Command winget -ErrorAction SilentlyContinue) {
  & winget install --id Gyan.FFmpeg --exact --accept-package-agreements --accept-source-agreements
  if ($LASTEXITCODE -ne 0) { throw "winget failed to install FFmpeg" }
} elseif (Get-Command choco -ErrorAction SilentlyContinue) {
  & choco install ffmpeg -y
  if ($LASTEXITCODE -ne 0) { throw "Chocolatey failed to install FFmpeg" }
} else {
  throw "FFmpeg is required. Install it with winget or Chocolatey and rerun setup."
}

Refresh-ProcessPath
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue) -or
    -not (Get-Command ffprobe -ErrorAction SilentlyContinue)) {
  throw "FFmpeg was installed but is not on PATH yet. Open a new PowerShell task and rerun setup."
}

Write-Host "FFmpeg/ffprobe installed"
