param(
  [ValidateSet("start", "stop", "restart", "status", "open", "logs")]
  [string]$Command = "start"
)
$ErrorActionPreference = "Stop"
$AppRoot = $PSScriptRoot
$HomeDir = if ($env:OPENCHATCUT_HOME) { $env:OPENCHATCUT_HOME } else { Join-Path $HOME ".openchatcut" }
$WebPort = if ($env:OPENCHATCUT_WEB_PORT) { [int]$env:OPENCHATCUT_WEB_PORT } else { 3100 }
$DaemonPort = 3210
if ($WebPort -lt 1 -or $WebPort -gt 65535 -or $WebPort -eq $DaemonPort) {
  throw "Web and daemon ports must be distinct values from 1 to 65535"
}
$Daemon = Join-Path $AppRoot "bin\openchatcutd.exe"
$JsRuntime = Join-Path $AppRoot "bin\js-runtime.exe"
$Worker = Join-Path $HomeDir "runtime\media-worker\Scripts\openchatcut-media-worker.exe"
$MgRuntime = Join-Path $AppRoot "runtime\mg-runtime\src\cli.mjs"
$DaemonPid = Join-Path $HomeDir "portable-daemon.pid"
$WebPid = Join-Path $HomeDir "portable-web.pid"
$DaemonLog = Join-Path $HomeDir "openchatcutd.log"
$WebLog = Join-Path $HomeDir "web.log"

function Test-Ready([string]$Url) {
  try { Invoke-WebRequest -UseBasicParsing $Url -TimeoutSec 1 | Out-Null; return $true }
  catch { return $false }
}
function Wait-Ready([string]$Url, [string]$Name) {
  for ($Attempt = 0; $Attempt -lt 240; $Attempt++) {
    if (Test-Ready $Url) { return }
    Start-Sleep -Milliseconds 250
  }
  throw "$Name did not become ready"
}
function Test-PidMatches([string]$PidFile, [string]$CommandFragment) {
  if (-not (Test-Path $PidFile)) { return $false }
  $RawProcessId = (Get-Content $PidFile -Raw).Trim()
  if ($RawProcessId -notmatch '^\d+$') { return $false }
  $OwnedProcess = Get-CimInstance Win32_Process -Filter "ProcessId = $RawProcessId" -ErrorAction SilentlyContinue
  if (-not $OwnedProcess -or -not $OwnedProcess.CommandLine) { return $false }
  return $OwnedProcess.CommandLine.IndexOf($CommandFragment, [StringComparison]::OrdinalIgnoreCase) -ge 0
}
function Start-App {
  New-Item -ItemType Directory -Force -Path $HomeDir | Out-Null
  foreach ($Path in @($Daemon, $JsRuntime, $Worker, $MgRuntime)) {
    if (-not (Test-Path $Path)) { throw "Portable runtime is incomplete. Run install.ps1 first: $Path" }
  }
  if (Test-Ready "http://127.0.0.1:$DaemonPort/health") {
    if (-not (Test-PidMatches $DaemonPid "openchatcutd.exe")) {
      throw "Port $DaemonPort is already served by another daemon; stop it before starting this portable installation."
    }
  } else {
    $env:OPENCHATCUT_HOME = $HomeDir
    $env:OPENCHATCUT_BIND = "127.0.0.1:$DaemonPort"
    $env:OPENCHATCUT_EDITOR_URL = "http://127.0.0.1:$WebPort"
    $env:OPENCHATCUT_MEDIA_WORKER = $Worker
    $env:OPENCHATCUT_MG_RUNTIME_CLI = $MgRuntime
    $env:OPENCHATCUT_NODE_COMMAND = $JsRuntime
    $Process = Start-Process $Daemon -WindowStyle Hidden -PassThru `
      -RedirectStandardOutput $DaemonLog -RedirectStandardError "$DaemonLog.err"
    Set-Content $DaemonPid $Process.Id
  }
  Wait-Ready "http://127.0.0.1:$DaemonPort/health" "daemon"
  if (Test-Ready "http://127.0.0.1:$WebPort/api/health") {
    if (-not (Test-PidMatches $WebPid "apps/web/server.js")) {
      throw "Port $WebPort is already served by another Web process; choose OPENCHATCUT_WEB_PORT."
    }
  } else {
    $env:PORT = $WebPort.ToString()
    $env:HOSTNAME = "127.0.0.1"
    $env:NODE_ENV = "production"
    $env:NEXT_TELEMETRY_DISABLED = "1"
    $Process = Start-Process $JsRuntime -ArgumentList "apps/web/server.js" `
      -WorkingDirectory (Join-Path $AppRoot "web") -WindowStyle Hidden -PassThru `
      -RedirectStandardOutput $WebLog -RedirectStandardError "$WebLog.err"
    Set-Content $WebPid $Process.Id
  }
  Wait-Ready "http://127.0.0.1:$WebPort/api/health" "web"
  Write-Host "OpenChatCut: http://127.0.0.1:$WebPort/projects"
}
function Stop-App {
  foreach ($Owned in @(
    @{ File = $WebPid; Fragment = "apps/web/server.js" },
    @{ File = $DaemonPid; Fragment = "openchatcutd.exe" }
  )) {
    if (Test-PidMatches $Owned.File $Owned.Fragment) {
      $ProcessId = [int](Get-Content $Owned.File -Raw).Trim()
      Stop-Process -Id $ProcessId -ErrorAction Stop
      Wait-Process -Id $ProcessId -Timeout 10 -ErrorAction SilentlyContinue
    }
    Remove-Item $Owned.File -Force -ErrorAction SilentlyContinue
  }
  Write-Host "OpenChatCut stopped"
}
switch ($Command) {
  "start" { Start-App }
  "stop" { Stop-App }
  "restart" { Stop-App; Start-App }
  "status" {
    if (Test-Ready "http://127.0.0.1:$DaemonPort/health") { "daemon: ready" } else { "daemon: offline" }
    if (Test-Ready "http://127.0.0.1:$WebPort/api/health") { "web: ready" } else { "web: offline" }
  }
  "open" { Start-App; Start-Process "http://127.0.0.1:$WebPort/projects" }
  "logs" { Get-Content $DaemonLog,$WebLog -Wait }
}
