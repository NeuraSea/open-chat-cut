param(
  [Parameter(Mandatory=$true)][string]$Version,
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$OutputDir = "dist"
)
$ErrorActionPreference = "Stop"
if ($Version -notmatch '^[0-9A-Za-z._+-]+$') { throw "Invalid release version" }
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Output = Join-Path $Root $OutputDir
$Name = "openchatcut-$Version-$Target"
$Stage = Join-Path $Output $Name
$Archive = Join-Path $Output "$Name.zip"
$Inputs = @(
  (Join-Path $Root "target\release\openchatcutd.exe"),
  (Join-Path $Root "apps\web\.next\standalone\apps\web\server.js"),
  (Join-Path $Root "apps\web\.next\static"),
  (Join-Path $Root "apps\web\public"),
  (Join-Path $Root "packages\mg-runtime\node_modules\@babel\parser")
)
foreach ($Input in $Inputs) { if (-not (Test-Path $Input)) { throw "Missing release input: $Input" } }
$RuntimeCommand = Get-Command bun -ErrorAction SilentlyContinue
if (-not $RuntimeCommand) { $RuntimeCommand = Get-Command node -ErrorAction SilentlyContinue }
if (-not $RuntimeCommand) { throw "Missing JavaScript runtime: install Bun or Node" }
$JsRuntime = $RuntimeCommand.Source
Remove-Item -Recurse -Force $Stage,$Archive -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "bin"),(Join-Path $Stage "web\apps\web\.next"),(Join-Path $Stage "runtime"),(Join-Path $Stage "scripts\release") | Out-Null
Copy-Item (Join-Path $Root "target\release\openchatcutd.exe") (Join-Path $Stage "bin\openchatcutd.exe")
Copy-Item $JsRuntime (Join-Path $Stage "bin\js-runtime.exe")
& (Join-Path $Stage "bin\js-runtime.exe") --version | Out-Null
if ($LASTEXITCODE -ne 0) { throw "JavaScript runtime is not relocatable after copying: $JsRuntime" }
Copy-Item -Recurse -Force (Join-Path $Root "apps\web\.next\standalone\*") (Join-Path $Stage "web")
Copy-Item -Recurse -Force (Join-Path $Root "apps\web\.next\static") (Join-Path $Stage "web\apps\web\.next\static")
Copy-Item -Recurse -Force (Join-Path $Root "apps\web\public") (Join-Path $Stage "web\apps\web\public")
Copy-Item -Recurse -Force (Join-Path $Root "packages\mg-runtime") (Join-Path $Stage "runtime\mg-runtime")
Copy-Item -Recurse -Force (Join-Path $Root "services") $Stage
Get-ChildItem (Join-Path $Stage "services") -Directory -Recurse -Force |
  Where-Object { $_.Name -in @("__pycache__", ".pytest_cache") } |
  Remove-Item -Recurse -Force
Get-ChildItem (Join-Path $Stage "services") -File -Recurse -Force -Include *.pyc,*.pyo |
  Remove-Item -Force
Copy-Item -Recurse -Force (Join-Path $Root "plugins") $Stage
@(
  (Join-Path $Stage "services\media-worker\tests"),
  (Join-Path $Stage "plugins\open-chat-cut\tests"),
  (Join-Path $Stage "runtime\mg-runtime\test")
) | Where-Object { Test-Path $_ } | Remove-Item -Recurse -Force
New-Item -ItemType Directory -Force -Path (Join-Path $Stage ".agents\plugins") | Out-Null
Copy-Item (Join-Path $Root ".agents\plugins\marketplace.json") (Join-Path $Stage ".agents\plugins\marketplace.json")
Copy-Item (Join-Path $Root "scripts\install-ffmpeg.ps1"),(Join-Path $Root "scripts\install-codex-plugin.ps1") (Join-Path $Stage "scripts")
Copy-Item (Join-Path $Root "scripts\release\build-manifest.py"),(Join-Path $Root "scripts\release\verify-release-bundle.py"),(Join-Path $Root "scripts\release\openchatcut-installed.ps1") (Join-Path $Stage "scripts\release")
Copy-Item (Join-Path $Root "LICENSE"),(Join-Path $Root "NOTICE.md") $Stage
Copy-Item -Recurse (Join-Path $Root "LICENSES") $Stage
Set-Content (Join-Path $Stage "VERSION") $Version
Copy-Item (Join-Path $Root "scripts\release\openchatcut-portable.ps1") (Join-Path $Stage "openchatcut.ps1")
Copy-Item (Join-Path $Root "scripts\release\install-release.ps1") (Join-Path $Stage "install.ps1")
python (Join-Path $Root "scripts\release\build-manifest.py") $Stage --version $Version --target $Target
python (Join-Path $Root "scripts\release\verify-release-bundle.py") $Stage
python (Join-Path $Root "scripts\release\create-zip.py") $Stage $Archive
if ($LASTEXITCODE -ne 0) { throw "Release zip creation failed" }
python (Join-Path $Root "scripts\release\verify-release-bundle.py") $Archive
"$((Get-FileHash $Archive -Algorithm SHA256).Hash.ToLower())  $([IO.Path]::GetFileName($Archive))" | Set-Content "$Archive.sha256"
Write-Host $Archive
