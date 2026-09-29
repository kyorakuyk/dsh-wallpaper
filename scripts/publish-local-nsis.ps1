<#
.SYNOPSIS
  Build, hash, optionally install and verify one local DSH Wallpaper NSIS release.

.DESCRIPTION
  Run from any directory with PowerShell 7:
    pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-local-nsis.ps1

  This is the distribution path that needs no certificate and no administrator:
  Tauri's NSIS target installs for the current user under
  %LOCALAPPDATA%\dsh-wallpaper, and autostart uses
  HKCU\Software\Microsoft\Windows\CurrentVersion\Run instead of a package
  StartupTask. Both were measured on this machine on 2026-09-30.

  The installer is unsigned on purpose, so a user who downloads it will see
  SmartScreen's "Windows protected your PC" prompt once. The SHA-256 printed at
  the end is what belongs next to the download link.

  The MSIX path (scripts/publish-local-msix.ps1) stays for a future Store
  listing. It is no longer required by any feature: the lock screen was the only
  thing that needed package identity, and the product no longer touches it.

  Gates mirror the MSIX publisher so both paths are held to the same bar:
  TypeScript types, the frontend and Bridge tests, and the Rust tests. The Lite
  edition adds its own cargo checks and the Lite bundle boundary script.
#>
[CmdletBinding()]
param(
  [ValidateSet('full', 'lite')]
  [string]$Edition = 'full',
  [switch]$SkipChecks,
  [switch]$PlanOnly,
  [switch]$Install,
  [switch]$NoLaunch
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$tauriRoot = Join-Path $repoRoot 'wallpaper\src-tauri'
$bundleRoot = Join-Path $tauriRoot 'target\release\bundle\nsis'
$installDir = Join-Path $env:LOCALAPPDATA 'dsh-wallpaper'
$installExe = Join-Path $installDir 'dsh-wallpaper.exe'
$confPath = Join-Path $tauriRoot $(if ($Edition -eq 'lite') { 'tauri.lite.conf.json' } else { 'tauri.conf.json' })

function Invoke-Stage([string]$FilePath, [string[]]$Arguments, [string]$Stage) {
  Write-Host "`n==> $Stage"
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$Stage 失败，退出码 $LASTEXITCODE。" }
}

$conf = Get-Content -LiteralPath $confPath -Raw | ConvertFrom-Json
$version = [string]$conf.version
Write-Host "==> 目标：$($conf.productName) $version（$Edition，NSIS）"

$checks = @(
  @{ File = 'pnpm'; Args = @('typecheck'); Stage = 'TypeScript 类型检查' }
  @{ File = 'pnpm'; Args = @('test'); Stage = '前端与 Bridge 测试' }
  @{ File = 'cargo'; Args = @('test', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--all-targets'); Stage = 'Rust 测试' }
)
if ($Edition -eq 'lite') {
  $checks += @{ File = 'cargo'; Args = @('check', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--no-default-features', '--features', 'lite', '--bin', 'dsh-wallpaper-lite'); Stage = 'Lite 目标编译检查' }
  $checks += @{ File = 'cargo'; Args = @('test', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--no-default-features', '--features', 'lite', '--lib'); Stage = 'Lite 目标测试' }
}

$buildArgs = if ($Edition -eq 'lite') {
  @('-C', 'wallpaper', 'tauri', 'build', '--config', 'src-tauri/tauri.lite.conf.json')
} else {
  @('-C', 'wallpaper', 'tauri', 'build')
}

if ($PlanOnly) {
  Write-Host "`n==> 计划（未执行）"
  foreach ($c in $checks) { Write-Host ("    " + $c.File + ' ' + ($c.Args -join ' ')) }
  if ($Edition -eq 'lite') {
    Write-Host '    pnpm build:lite'
    Write-Host '    pwsh -File scripts/verify-lite-bundle.ps1'
  }
  Write-Host ("    pnpm " + ($buildArgs -join ' '))
  Write-Host "    产物：$bundleRoot\*-setup.exe（取最新一个）"
  if ($Install) {
    Write-Host "    安装：静默 /S 到 $installDir，$(if ($NoLaunch) { '随后不启动' } else { '随后启动' })"
  }
  return
}

if (-not $SkipChecks) {
  foreach ($c in $checks) { Invoke-Stage $c.File $c.Args $c.Stage }
}

if ($Edition -eq 'lite') {
  Invoke-Stage 'pnpm' @('build:lite') 'Lite 前端构建'
  Invoke-Stage 'pwsh' @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $repoRoot 'scripts\verify-lite-bundle.ps1')) 'Lite 边界检查'
}

Invoke-Stage 'pnpm' $buildArgs 'Rust Release 构建 + NSIS 打包'

$installer = Get-ChildItem -LiteralPath $bundleRoot -Filter '*-setup.exe' -File -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) {
  throw "构建完成，但 $bundleRoot 下没有 setup.exe。请确认 tauri 配置里 bundle.targets 含 nsis。"
}

$signature = (Get-AuthenticodeSignature -LiteralPath $installer.FullName).Status
$sha256 = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash

$installedVersion = $null
if ($Install) {
  Get-Process -Name 'dsh-wallpaper' -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "    停止 pid=$($_.Id)"
    Stop-Process -Id $_.Id -Force
  }
  Start-Sleep -Seconds 2
  Write-Host "`n==> 静默安装（当前用户，不请求管理员）"
  $p = Start-Process -FilePath $installer.FullName -ArgumentList '/S' -PassThru -Wait
  if ($p.ExitCode -ne 0) { throw "安装器退出码 $($p.ExitCode)。" }
  if (-not (Test-Path -LiteralPath $installExe -PathType Leaf)) { throw "安装完成，但找不到 $installExe。" }
  $installedVersion = (Get-Item -LiteralPath $installExe).VersionInfo.FileVersion
  if ($installedVersion -ne $version) {
    throw "装上的版本是 $installedVersion，而目标是 $version。"
  }
  if (-not $NoLaunch) { Start-Process -FilePath $installExe | Out-Null }
}

[pscustomobject]@{
  Status        = 'published'
  Edition       = $Edition
  Version       = $version
  Installer     = $installer.FullName
  SizeMB        = [math]::Round($installer.Length / 1MB, 1)
  Signature     = $signature
  Sha256        = $sha256
  InstalledExe  = if ($installedVersion) { $installExe } else { $null }
  Installed     = $installedVersion
} | Format-List
