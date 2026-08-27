<# Builds and installs the isolated MSIX lock-screen probe. #>
[CmdletBinding()]
param([switch]$Install)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $root 'wallpaper\src-tauri'
$artifactRoot = Join-Path $root 'artifacts\lockscreen-probe'
$workRoot = Join-Path $artifactRoot ([guid]::NewGuid().ToString('N'))
$layout = Join-Path $workRoot 'layout'
$manifest = Join-Path $root 'packaging\msix\LockScreenProbe.AppxManifest.xml'

function SdkTool([string]$name) {
  $root = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  return Get-ChildItem -LiteralPath $root -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } | Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\$name" } |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}

$makeAppx = SdkTool 'MakeAppx.exe'
$signTool = SdkTool 'SignTool.exe'
if (-not $makeAppx -or -not $signTool) { throw '需要安装 Windows SDK（MakeAppx.exe 与 SignTool.exe）。' }

Push-Location $tauriRoot
try { cargo build --release --locked --features lockscreen-probe --bin lockscreen_probe; if ($LASTEXITCODE) { throw '诊断程序编译失败。' } }
finally { Pop-Location }

New-Item -ItemType Directory -Path (Join-Path $layout 'Assets') -Force | Out-Null
Copy-Item -LiteralPath $manifest -Destination (Join-Path $layout 'AppxManifest.xml')
Copy-Item -LiteralPath (Join-Path $tauriRoot 'target\release\lockscreen_probe.exe') -Destination $layout
Copy-Item -LiteralPath (Join-Path $root 'wallpaper\public\personas\wake-frames\variant-anima\sleep.png') -Destination (Join-Path $layout 'Assets\sleep.png')
foreach ($name in @('StoreLogo.png', 'Square150x150Logo.png', 'Square44x44Logo.png')) {
  Copy-Item -LiteralPath (Join-Path $root 'wallpaper\src-tauri\icons\icon.png') -Destination (Join-Path $layout "Assets\$name")
}

New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
$package = Join-Path $artifactRoot 'dsh-wallpaper-lockscreen-probe.msix'
if (Test-Path -LiteralPath $package) { Remove-Item -LiteralPath $package -Force }
& $makeAppx pack /o /h SHA256 /d $layout /p $package
if ($LASTEXITCODE) { throw '最小诊断包打包失败。' }

$certificate = Get-ChildItem Cert:\CurrentUser\My |
  Where-Object { $_.Subject -eq 'CN=DSH Wallpaper Test' -and $_.HasPrivateKey } |
  Sort-Object NotBefore -Descending | Select-Object -First 1
if (-not $certificate) { throw '找不到当前测试签名证书；请先构建并安装 dsh-wallpaper 的 MSIX 测试包。' }
& $signTool sign /fd SHA256 /sha1 $certificate.Thumbprint $package
if ($LASTEXITCODE) { throw '最小诊断包签名失败。' }

if ($Install) { Add-AppxPackage -Path $package }
Write-Host "诊断包：$package"
Write-Host '报告：%LOCALAPPDATA%\DSHWallpaperLockScreenProbe\report.json'
