[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$distRoot = Join-Path $repoRoot 'wallpaper\dist-lite'
$forbiddenStrings = @(
  'DeepSeek Harness', '会话生命周期', 'ConversationBubble', 'conversation-shell',
  'send_chat', 'harness_presets', 'deepseek-web', 'dsh-wallpaper:conversations',
  'chatOpen', 'harnessOnline'
)
$webFiles = @(Get-ChildItem -LiteralPath $distRoot -Recurse -File |
  Where-Object { $_.Extension -in '.js', '.css', '.html' })

foreach ($needle in $forbiddenStrings) {
  $match = $webFiles | Select-String -Pattern $needle -SimpleMatch -List
  if ($match) { throw "Lite 产物包含禁止内容：$needle" }
}

$requiredFiles = @(
  'index.html',
  'personas\wake-frames\variant-anima\sleep.png',
  'personas\wake-frames\variant-anima\frame-2-eyes.png',
  'personas\wake-frames\variant-anima\frame-3-situp.png',
  'personas\wake-frames\variant-anima\frame-4-yawn.png'
)
foreach ($relativePath in $requiredFiles) {
  $path = Join-Path $distRoot $relativePath
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Lite 产物缺少正式资源：$path"
  }
}

$capabilityPath = Join-Path $repoRoot 'wallpaper\src-tauri\capabilities\lite-background.json'
$liteCapability = Get-Content -LiteralPath $capabilityPath -Raw | ConvertFrom-Json
$forbiddenPermissions = @(
  'allow-open-settings-window', 'allow-send-chat', 'allow-cancel-chat',
  'allow-connect-harness', 'allow-probe-harness',
  'allow-begin-interaction-region-session', 'allow-update-interaction-regions'
)
foreach ($permission in $forbiddenPermissions) {
  if ($liteCapability.permissions -contains $permission) {
    throw "Lite 背景权限包含完整版能力：$permission"
  }
}

Write-Host "Lite bundle boundary: OK ($($webFiles.Count) web files, $($requiredFiles.Count) required resources)."
