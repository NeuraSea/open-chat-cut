$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Marketplace = "openchatcut-local"
$Selector = "open-chat-cut@$Marketplace"

if (-not (Get-Command codex -ErrorAction SilentlyContinue)) {
  throw "Codex CLI is required. Install Codex and run 'codex login' first."
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js is required by the bundled STDIO bridge. Install Node.js 20+ and retry."
}

function Invoke-Native {
  param([string]$Command, [string[]]$Arguments)
  & $Command @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Command failed with exit code $LASTEXITCODE"
  }
}

$PluginTests = Get-ChildItem (Join-Path $Root "plugins\open-chat-cut\tests") -Filter "*.test.mjs" | ForEach-Object { $_.FullName }
Invoke-Native node (@("--test") + $PluginTests)
Invoke-Native node @((Join-Path $Root "plugins\open-chat-cut\mcp\check-runtime.mjs"))
try { codex plugin remove $Selector --json 2>$null | Out-Null } catch {}
try { codex plugin marketplace remove $Marketplace 2>$null | Out-Null } catch {}
Invoke-Native codex @("plugin", "marketplace", "add", $Root, "--json")
Invoke-Native codex @("plugin", "add", $Selector, "--json")
Write-Host "`nOpenChatCut Codex plugin installed. Open a new Codex task before using it."
