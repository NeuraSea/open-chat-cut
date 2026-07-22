param(
  [ValidateSet("start", "stop", "restart", "status", "logs")]
  [string]$Command = "start"
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$HomeDir = if ($env:OPENCHATCUT_HOME) { $env:OPENCHATCUT_HOME } else { Join-Path $HOME ".openchatcut" }
$PidFile = Join-Path $HomeDir "launcher.pid"
$LogFile = Join-Path $HomeDir "openchatcutd.log"
$Daemon = Join-Path $Root "target\release\openchatcutd.exe"
$VideoAcceleration = if ($env:OPENCHATCUT_VIDEO_ACCELERATION) { $env:OPENCHATCUT_VIDEO_ACCELERATION.ToLowerInvariant() } else { "auto" }
$WebPortText = if ($env:OPENCHATCUT_WEB_PORT) { $env:OPENCHATCUT_WEB_PORT } else { "3100" }
$AuthorizedImportRoot = if ($env:OPENCHATCUT_AUTHORIZED_IMPORT_ROOT) { $env:OPENCHATCUT_AUTHORIZED_IMPORT_ROOT } else { $null }
$WebPort = 0
if (-not [int]::TryParse($WebPortText, [ref]$WebPort) -or $WebPort -lt 1 -or $WebPort -gt 65535 -or $WebPort -eq 3210) {
  throw "OPENCHATCUT_WEB_PORT must be from 1 to 65535 and cannot be the daemon port 3210"
}
if ($VideoAcceleration -notin @("auto", "cpu", "apple", "nvidia")) {
  throw "OPENCHATCUT_VIDEO_ACCELERATION must be auto, cpu, apple, or nvidia"
}

function Test-Daemon {
  try { Invoke-WebRequest -UseBasicParsing http://127.0.0.1:3210/health -TimeoutSec 1 | Out-Null; return $true }
  catch { return $false }
}

function Start-OpenChatCut {
  New-Item -ItemType Directory -Force -Path $HomeDir | Out-Null
  if (-not (Test-Daemon)) {
    if (-not (Test-Path $Daemon)) { throw "Daemon is not built. Run .\scripts\setup.ps1 first." }
    $env:OPENCHATCUT_HOME = $HomeDir
    $env:OPENCHATCUT_MEDIA_WORKER = Join-Path $Root ".venv\Scripts\openchatcut-media-worker.exe"
    $env:OPENCHATCUT_MG_RUNTIME_CLI = Join-Path $Root "packages\mg-runtime\src\cli.mjs"
    $env:OPENCHATCUT_VIDEO_ACCELERATION = $VideoAcceleration
    $env:OPENCHATCUT_EDITOR_URL = "http://127.0.0.1:$WebPort"
    $DaemonArguments = @()
    if ($AuthorizedImportRoot) {
      $ResolvedImportRoot = (Resolve-Path -Path $AuthorizedImportRoot).Path
      if (-not (Test-Path -Path $ResolvedImportRoot -PathType Container)) {
        throw "OPENCHATCUT_AUTHORIZED_IMPORT_ROOT must identify an existing directory"
      }
      $DaemonArguments = @("--authorized-import-root", $ResolvedImportRoot)
    }
    $Process = Start-Process -FilePath $Daemon -ArgumentList $DaemonArguments -RedirectStandardOutput $LogFile -RedirectStandardError "$LogFile.err" -PassThru -WindowStyle Hidden
    Set-Content -Path $PidFile -Value $Process.Id
  }
  # Capability probing can take longer than 15 seconds on a cold worker start.
  for ($Attempt = 0; $Attempt -lt 240 -and -not (Test-Daemon); $Attempt++) { Start-Sleep -Milliseconds 250 }
  if (-not (Test-Daemon)) { throw "Daemon did not become ready; see $LogFile" }
  Push-Location $Root
  try {
    $env:OPENCHATCUT_WEB_PORT = $WebPort.ToString()
    docker compose up -d web
  } finally { Pop-Location }
  Write-Host "OpenChatCut editor: http://127.0.0.1:$WebPort/projects"
}

function Stop-OpenChatCut {
  Push-Location $Root
  try { docker compose stop web 2>$null | Out-Null } finally { Pop-Location }
  if (Test-Path $PidFile) {
    $DaemonPid = Get-Content $PidFile
    Stop-Process -Id $DaemonPid -ErrorAction SilentlyContinue
    Remove-Item $PidFile -Force
  }
}

switch ($Command) {
  "start" { Start-OpenChatCut }
  "stop" { Stop-OpenChatCut }
  "restart" { Stop-OpenChatCut; Start-OpenChatCut }
  "status" {
    if (Test-Daemon) { Write-Host "daemon: ready" }
    else { Write-Host "daemon: offline" }
  }
  "logs" { Get-Content -Path $LogFile -Wait }
}
