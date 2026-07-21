param([switch]$WithoutMl, [switch]$SkipPlugin, [switch]$NoStart)
$ErrorActionPreference = "Stop"
$Arguments = @()
if ($WithoutMl) { $Arguments += "-WithoutMl" }
& (Join-Path $PSScriptRoot "setup.ps1") @Arguments
if (-not $SkipPlugin) {
  if (Get-Command codex -ErrorAction SilentlyContinue) {
    & (Join-Path $PSScriptRoot "install-codex-plugin.ps1")
  } else {
    Write-Warning "Codex CLI is not installed; plugin installation was skipped."
  }
}
if (-not $NoStart) { & (Join-Path $PSScriptRoot "openchatcut.ps1") start }
Write-Host "OpenChatCut source installation is ready."
