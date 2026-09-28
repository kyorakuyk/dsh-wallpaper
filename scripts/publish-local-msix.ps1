<#
.SYNOPSIS
  Build, sign, install, launch, and verify one local DSH Wallpaper MSIX release.

.DESCRIPTION
  Run from any directory with PowerShell 7:
    pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\publish-local-msix.ps1

  The existing MSIX builder performs the frontend and Release Rust builds. This
  wrapper runs the project checks, advances the package revision when needed,
  saves the installed package for rollback, signs with a private key already in
  Cert:\CurrentUser\My, imports only the matching public CER into
  LocalMachine\TrustedPeople when needed, installs for the current user, and
  verifies the running executable and bundled portrait assets.

  Signing does not read or export a PFX and never passes a private-key password
  on a command line. UAC is requested only for the public-certificate trust
  import, which requires administrator rights. Declining UAC stops before install.
#>
[CmdletBinding()]
param(
  [ValidateSet('full', 'lite')]
  [string]$Edition = 'full',
  [string]$PackageVersion,
  [string]$CertificateThumbprint,
  [string]$PublicCertificatePath,
  [switch]$PlanOnly,
  [switch]$SkipChecks,
  [switch]$NoForceApplicationShutdown,
  [switch]$NoLaunch,
  [switch]$ImportTrustOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$script:ArtifactRoot = Join-Path $script:RepoRoot 'artifacts\msix-test'
$script:ManifestName = if ($Edition -eq 'lite') { 'AppxManifest-Lite.xml' } else { 'AppxManifest.xml' }
$script:BinaryName = if ($Edition -eq 'lite') { 'dsh-wallpaper-lite' } else { 'dsh-wallpaper' }
$script:ManifestPath = Join-Path (Join-Path $script:RepoRoot 'packaging\msix') $script:ManifestName
$script:BuildScript = Join-Path $script:RepoRoot 'scripts\build-msix-test.ps1'
$script:LiteBoundaryScript = Join-Path $script:RepoRoot 'scripts\verify-lite-bundle.ps1'
$msixFileName = if ($Edition -eq 'lite') { 'dsh-wallpaper-lite-lockscreen-test.msix' } else { 'dsh-wallpaper-lockscreen-test.msix' }
$script:MsixPath = Join-Path $script:ArtifactRoot $msixFileName
if (-not $PublicCertificatePath) {
  $PublicCertificatePath = Join-Path $script:ArtifactRoot 'dsh-wallpaper-test.cer'
}
$PublicCertificatePath = [IO.Path]::GetFullPath($PublicCertificatePath)

function Get-PackageIdentityFromText([string]$ManifestText) {
  $document = [Xml.XmlDocument]::new()
  $document.LoadXml($ManifestText)
  $identity = $document.SelectSingleNode("/*[local-name()='Package']/*[local-name()='Identity']")
  if (-not $identity) { throw 'MSIX 清单中没有 Package Identity。' }
  $versionText = $identity.GetAttribute('Version')
  if ($versionText -notmatch '^\d+\.\d+\.\d+\.\d+$') {
    throw "MSIX 版本必须为四段数字：$versionText"
  }
  return [pscustomobject]@{
    Name = $identity.GetAttribute('Name')
    Publisher = $identity.GetAttribute('Publisher')
    VersionText = $versionText
    Version = [version]$versionText
  }
}

function Get-PackageIdentityFromFile([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "找不到 MSIX 清单：$Path" }
  $text = [IO.File]::ReadAllText($Path)
  return Get-PackageIdentityFromText $text
}

function Get-PackageIdentityFromMsix([string]$Path) {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($Path)
  try {
    $entry = $archive.GetEntry('AppxManifest.xml')
    if (-not $entry) { throw 'MSIX 中缺少 AppxManifest.xml。' }
    $stream = $entry.Open()
    try {
      $reader = [IO.StreamReader]::new($stream)
      try {
        $text = $reader.ReadToEnd()
        return Get-PackageIdentityFromText $text
      }
      finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
  } finally { $archive.Dispose() }
}

function Get-InstalledPackage([string]$PackageName) {
  $packages = @(Get-AppxPackage -Name $PackageName -ErrorAction SilentlyContinue)
  if ($packages.Count -eq 0) { return $null }
  return $packages | Sort-Object { [version]$_.Version } -Descending | Select-Object -First 1
}

function Get-TargetVersion([version]$SourceVersion, $Installed, [string]$RequestedVersion) {
  if ($RequestedVersion) {
    if ($RequestedVersion -notmatch '^\d+\.\d+\.\d+\.\d+$') {
      throw "-PackageVersion 必须是四段版本号，例如 0.2.0.75；收到：$RequestedVersion"
    }
    $requested = [version]$RequestedVersion
    if ($Installed -and $requested -le [version]$Installed.Version) {
      throw "指定版本 $requested 必须高于已安装版本 $($Installed.Version)。"
    }
    return $requested
  }
  if ($Installed -and $SourceVersion -le [version]$Installed.Version) {
    $installedVersion = [version]$Installed.Version
    $revision = [Math]::Max(0, $installedVersion.Revision) + 1
    if ($revision -gt 65535) { throw "版本修订号已达到上限：$installedVersion" }
    return [version]::new($installedVersion.Major, $installedVersion.Minor, $installedVersion.Build, $revision)
  }
  return $SourceVersion
}

function New-VersionedManifest([string]$Path, [string]$OldVersion, [string]$NewVersion) {
  $text = [IO.File]::ReadAllText($Path)
  if ($OldVersion -ne $NewVersion) {
    $oldAttribute = 'Version="' + $OldVersion + '"'
    $newAttribute = 'Version="' + $NewVersion + '"'
    $count = [regex]::Matches($text, [regex]::Escape($oldAttribute)).Count
    if ($count -ne 1) { throw "清单中预期恰好一个版本字段，找到 $count 个。" }
    $text = $text.Replace($oldAttribute, $newAttribute)
  }
  New-Item -ItemType Directory -Path $script:ArtifactRoot -Force | Out-Null
  $temporaryPath = Join-Path $script:ArtifactRoot ("publish-manifest-$Edition-$([guid]::NewGuid().ToString('N')).xml")
  [IO.File]::WriteAllText($temporaryPath, $text, [Text.UTF8Encoding]::new($false))
  return $temporaryPath
}

function Test-Administrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-MachineTrustedCertificate([string]$Thumbprint) {
  $store = [Security.Cryptography.X509Certificates.X509Store]::new(
    [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
    [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
  )
  try {
    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
    return @($store.Certificates | Where-Object { $_.Thumbprint -eq $Thumbprint }).Count -gt 0
  } finally { $store.Close() }
}

function Import-PublicCertificate([string]$Path, [string]$Thumbprint, [string]$Publisher) {
  if (-not (Test-Administrator)) { throw '导入到 LocalMachine\TrustedPeople 需要管理员权限。' }
  $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($Path)
  try {
    if ($certificate.Thumbprint -ne $Thumbprint) { throw 'CER 指纹与本次签名证书不一致。' }
    if ($certificate.Subject.Trim() -cne $Publisher.Trim()) { throw 'CER Subject 与 MSIX Publisher 不一致。' }
    $now = Get-Date
    if ($certificate.NotBefore -gt $now -or $certificate.NotAfter -lt $now) { throw '测试签名证书当前不在有效期内。' }
    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
      [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
      [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
      $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
      if (@($store.Certificates | Where-Object { $_.Thumbprint -eq $Thumbprint }).Count -eq 0) {
        $store.Add($certificate)
      }
    } finally { $store.Close() }
  } finally { $certificate.Dispose() }
  Write-Host "已将匹配的公开证书导入 LocalMachine\TrustedPeople：$Thumbprint"
}

function Quote-ProcessArgument([string]$Value) {
  $builder = [Text.StringBuilder]::new()
  [void]$builder.Append('"')
  $backslashCount = 0
  foreach ($character in $Value.ToCharArray()) {
    if ([int]$character -eq 92) {
      $backslashCount++
      continue
    }
    if ([int]$character -eq 34) {
      [void]$builder.Append([string]::new([char]92, (2 * $backslashCount + 1)))
      [void]$builder.Append('"')
    } else {
      if ($backslashCount -gt 0) {
        [void]$builder.Append([string]::new([char]92, $backslashCount))
      }
      [void]$builder.Append($character)
    }
    $backslashCount = 0
  }
  if ($backslashCount -gt 0) {
    [void]$builder.Append([string]::new([char]92, (2 * $backslashCount)))
  }
  [void]$builder.Append('"')
  return $builder.ToString()
}

function Ensure-MachineTrust([string]$CerPath, [string]$Thumbprint, [string]$Publisher) {
  if (Test-MachineTrustedCertificate $Thumbprint) {
    Write-Host "机器已信任签名证书：$Thumbprint"
    return
  }
  if (Test-Administrator) {
    Import-PublicCertificate $CerPath $Thumbprint $Publisher
  } else {
    Write-Host '机器尚未信任该测试证书；现在将只提升公开 CER 的导入步骤，Windows 会显示 UAC 确认。'
    $hostExecutable = (Get-Process -Id $PID).Path
    $arguments = @(
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Quote-ProcessArgument $PSCommandPath),
      '-ImportTrustOnly', '-Edition', $Edition,
      '-PublicCertificatePath', (Quote-ProcessArgument $CerPath),
      '-CertificateThumbprint', $Thumbprint
    ) -join ' '
    try {
      $elevated = Start-Process -FilePath $hostExecutable -Verb RunAs -ArgumentList $arguments -Wait -PassThru -ErrorAction Stop
    } catch {
      throw "UAC 提权未获批准；证书和软件包均未安装。$($_.Exception.Message)"
    }
    if ($elevated.ExitCode -ne 0) { throw "公开证书导入失败，提升进程退出码：$($elevated.ExitCode)" }
  }
  if (-not (Test-MachineTrustedCertificate $Thumbprint)) { throw '证书导入后仍未出现在 LocalMachine\TrustedPeople。' }
}

function Resolve-WindowsSdkTool([string]$ToolName) {
  $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  if (-not (Test-Path -LiteralPath $sdkRoot -PathType Container)) { throw '找不到 Windows SDK 工具目录。' }
  $tool = Get-ChildItem -LiteralPath $sdkRoot -Directory |
    Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\$ToolName" } |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
    Select-Object -First 1
  if (-not $tool) { throw "Windows SDK 中找不到 x64\$ToolName。" }
  return $tool
}

function Invoke-CheckedCommand([string]$FilePath, [string[]]$Arguments, [string]$Stage) {
  Write-Host "`n==> $Stage"
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$Stage 失败，退出码 $LASTEXITCODE。" }
}

function Get-RollbackDirectory($Installed, [string]$PackageArtifact, [string]$CerPath, [string]$PackageName, [string]$Publisher, [string]$SignerThumbprint) {
  if (-not $Installed) { return $null }
  if (-not (Test-Path -LiteralPath $PackageArtifact -PathType Leaf)) {
    throw "当前版本 $($Installed.Version) 已安装，但找不到对应 MSIX，无法先保存回滚包：$PackageArtifact"
  }
  $oldIdentity = Get-PackageIdentityFromMsix $PackageArtifact
  if ($oldIdentity.Name -ne $PackageName -or $oldIdentity.Publisher.Trim() -cne $Publisher.Trim() -or $oldIdentity.Version -ne [version]$Installed.Version) {
    throw "现有 MSIX 与已安装版本不匹配；为避免丢失回滚点而停止。MSIX=$($oldIdentity.Version)，安装=$($Installed.Version)"
  }
  $oldSignature = Get-AuthenticodeSignature -LiteralPath $PackageArtifact
  if ($oldSignature.Status -ne 'Valid' -or $oldSignature.SignerCertificate.Thumbprint -ne $SignerThumbprint) {
    throw "当前回滚 MSIX 的签名无效或证书不同；拒绝覆盖：$PackageArtifact"
  }
  $baseName = 'rollback-' + ([version]$Installed.Version).ToString(4)
  $rollback = Join-Path $script:ArtifactRoot $baseName
  $existing = Join-Path $rollback 'dsh-wallpaper.msix'
  if (Test-Path -LiteralPath $rollback) {
    if ((Test-Path -LiteralPath $existing) -and
        (Get-FileHash -LiteralPath $existing -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $PackageArtifact -Algorithm SHA256).Hash) {
      return $rollback
    }
    $rollback = Join-Path $script:ArtifactRoot ($baseName + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
  }
  $artifactPrefix = $script:ArtifactRoot.TrimEnd('\') + '\'
  if (-not $rollback.StartsWith($artifactPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Rollback 目标超出了 artifacts/msix-test。'
  }
  New-Item -ItemType Directory -Path $rollback | Out-Null
  Copy-Item -LiteralPath $PackageArtifact -Destination (Join-Path $rollback 'dsh-wallpaper.msix')
  Copy-Item -LiteralPath $CerPath -Destination (Join-Path $rollback 'dsh-wallpaper-test.cer')
  if ((Get-FileHash -LiteralPath $PackageArtifact -Algorithm SHA256).Hash -ne
      (Get-FileHash -LiteralPath (Join-Path $rollback 'dsh-wallpaper.msix') -Algorithm SHA256).Hash) {
    throw '回滚包备份哈希不匹配。'
  }
  return $rollback
}

function Assert-MsixPortraits([string]$Path) {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $mapping = @(
    @('assets\personas\蓝幼.png', 'dist/personas/portrait-blue-child.png'),
    @('assets\personas\蓝熟.png', 'dist/personas/portrait-blue-adult.png'),
    @('assets\personas\黑红幼.png', 'dist/personas/portrait-black-child.png'),
    @('assets\personas\黑红熟.png', 'dist/personas/portrait-black-adult.png')
  )
  $archive = [IO.Compression.ZipFile]::OpenRead($Path)
  try {
    $hashAlgorithm = [Security.Cryptography.SHA256]::Create()
    try {
      foreach ($pair in $mapping) {
        $sourcePath = Join-Path $script:RepoRoot $pair[0]
        $entry = $archive.GetEntry($pair[1])
        if (-not $entry) { throw "MSIX 缺少立绘资源：$($pair[1])" }
        $sourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash
        $stream = $entry.Open()
        try { $packagedHash = [Convert]::ToHexString($hashAlgorithm.ComputeHash($stream)) }
        finally { $stream.Dispose() }
        if ($sourceHash -ne $packagedHash) { throw "MSIX 立绘与源图不一致：$($pair[0])" }
      }
    } finally { $hashAlgorithm.Dispose() }
  } finally { $archive.Dispose() }
}

function Assert-InstalledPortraits([string]$InstallLocation) {
  $mapping = @(
    @('assets\personas\蓝幼.png', 'portrait-blue-child.png'),
    @('assets\personas\蓝熟.png', 'portrait-blue-adult.png'),
    @('assets\personas\黑红幼.png', 'portrait-black-child.png'),
    @('assets\personas\黑红熟.png', 'portrait-black-adult.png')
  )
  foreach ($pair in $mapping) {
    $source = Join-Path $script:RepoRoot $pair[0]
    $installed = Join-Path $InstallLocation (Join-Path 'dist\personas' $pair[1])
    if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) { throw "安装目录缺少立绘：$($pair[1])" }
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne
        (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash) {
      throw "安装目录立绘与源图不一致：$($pair[1])"
    }
  }
}

function Get-RunningPackageProcesses([string]$InstallLocation) {
  $prefix = $InstallLocation.TrimEnd('\') + '\'
  $processName = $script:BinaryName + '.exe'
  return @(Get-CimInstance Win32_Process -Filter "name='$processName'" -ErrorAction SilentlyContinue |
    Where-Object { $_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) })
}

$identity = Get-PackageIdentityFromFile $script:ManifestPath
if (-not (Test-Path -LiteralPath $PublicCertificatePath -PathType Leaf)) {
  throw "找不到公开签名证书 CER：$PublicCertificatePath"
}
$publicCertificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($PublicCertificatePath)
if ($publicCertificate.Subject.Trim() -cne $identity.Publisher.Trim()) { throw 'CER Subject 与 MSIX Publisher 不一致。' }
$thumbprint = ($publicCertificate.Thumbprint -replace '\s', '').ToUpperInvariant()
if ($CertificateThumbprint) {
  if (($CertificateThumbprint -replace '\s', '').ToUpperInvariant() -ne $thumbprint) { throw '指定指纹与 CER 不一致。' }
}
$now = Get-Date
if ($publicCertificate.NotBefore -gt $now -or $publicCertificate.NotAfter -lt $now) { throw '测试签名证书当前不在有效期内。' }

if ($ImportTrustOnly) {
  Import-PublicCertificate $PublicCertificatePath $thumbprint $identity.Publisher
  exit 0
}

$signingCertificates = @(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My' -ErrorAction SilentlyContinue |
  Where-Object { $_.Thumbprint -eq $thumbprint -and $_.HasPrivateKey })
if ($signingCertificates.Count -ne 1) {
  throw "CurrentUser\My 中需要恰好一个与 CER 匹配且带私钥的证书：$thumbprint"
}
$signingCertificate = $signingCertificates[0]
if ($signingCertificate.NotBefore -gt $now -or $signingCertificate.NotAfter -lt $now) { throw '签名私钥证书当前不在有效期内。' }

$installedPackages = @(Get-AppxPackage -Name $identity.Name -ErrorAction SilentlyContinue)
$installedPackage = $installedPackages | Sort-Object { [version]$_.Version } -Descending | Select-Object -First 1
$installedVersion = if ($installedPackage) { [version]$installedPackage.Version } else { $null }
$targetVersion = Get-TargetVersion $identity.Version $installedPackage $PackageVersion

$windowsSdkSignTool = Resolve-WindowsSdkTool 'signtool.exe'
$windowsSdkMakeAppx = Resolve-WindowsSdkTool 'makeappx.exe'
$trusted = Test-MachineTrustedCertificate $thumbprint
$running = @()
if ($installedPackage) { $running = @(Get-RunningPackageProcesses $installedPackage.InstallLocation) }
if ($NoForceApplicationShutdown -and $running.Count -gt 0 -and -not $PlanOnly) {
  throw '壁纸仍在运行；请先从托盘正常退出，或移除 -NoForceApplicationShutdown 后重试。'
}

if ($PlanOnly) {
  $rollbackArtifactMatches = $false
  if ($installedPackage -and (Test-Path -LiteralPath $script:MsixPath -PathType Leaf)) {
    try {
      $previousIdentity = Get-PackageIdentityFromMsix $script:MsixPath
      $previousSignature = Get-AuthenticodeSignature -LiteralPath $script:MsixPath
      $rollbackArtifactMatches = $previousIdentity.Name -eq $identity.Name -and
        $previousIdentity.Publisher.Trim() -ceq $identity.Publisher.Trim() -and
        $previousIdentity.Version -eq $installedVersion -and
        $previousSignature.Status -eq 'Valid' -and
        $previousSignature.SignerCertificate.Thumbprint -eq $thumbprint
    } catch { $rollbackArtifactMatches = $false }
  }
  [pscustomobject]@{
    Edition = $Edition
    PackageName = $identity.Name
    PackageArtifact = $script:MsixPath
    InstalledVersion = if ($installedVersion) { $installedVersion.ToString(4) } else { '未安装' }
    TargetVersion = $targetVersion.ToString(4)
    SignerThumbprint = $thumbprint
    PrivateKeyAvailable = $true
    MachineTrustPresent = $trusted
    UacNeededForTrustImport = -not $trusted -and -not (Test-Administrator)
    RunningPackageProcesses = $running.Count
    WillForceClose = $running.Count -gt 0 -and -not $NoForceApplicationShutdown
    VerifiedRollbackArtifactAvailable = $rollbackArtifactMatches
    WillRunChecks = -not $SkipChecks
    WillRunLiteChecks = $Edition -eq 'lite' -and -not $SkipChecks
    WillVerifyLiteBundle = $Edition -eq 'lite'
    WillLaunchAfterInstall = -not $NoLaunch
    SignTool = $windowsSdkSignTool
    MakeAppx = $windowsSdkMakeAppx
  } | Format-List
  return
}

$git = Get-Command git -ErrorAction SilentlyContinue
if ($git) {
  $dirty = @(& $git.Source -C $script:RepoRoot status --short)
  if ($dirty.Count -gt 0) {
    Write-Warning '工作树有未提交改动；本次包会包含这些改动：'
    $dirty | ForEach-Object { Write-Host "  $_" }
  }
}

if (-not $SkipChecks) {
  Push-Location $script:RepoRoot
  try {
    Invoke-CheckedCommand 'pnpm' @('typecheck') 'TypeScript 类型检查'
    Invoke-CheckedCommand 'pnpm' @('test') '前端与 Bridge 测试'
    Invoke-CheckedCommand 'cargo' @('test', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--all-targets') 'Rust 测试'
    if ($Edition -eq 'lite') {
      Invoke-CheckedCommand 'cargo' @('check', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--no-default-features', '--features', 'lite', '--bin', 'dsh-wallpaper-lite') 'Lite 原生目标检查'
      Invoke-CheckedCommand 'cargo' @('test', '--manifest-path', 'wallpaper/src-tauri/Cargo.toml', '--locked', '--no-default-features', '--features', 'lite', '--lib') 'Lite 原生测试'
    }
  } finally { Pop-Location }
} else {
  Write-Warning '已跳过自动化检查。'
}

$rollbackDirectory = Get-RollbackDirectory $installedPackage $script:MsixPath $PublicCertificatePath $identity.Name $identity.Publisher $thumbprint
if ($rollbackDirectory) { Write-Host "上一版回滚包：$rollbackDirectory" }
$versionedManifestPath = New-VersionedManifest $script:ManifestPath $identity.VersionText $targetVersion.ToString(4)
if ($targetVersion -ne $identity.Version) {
  Write-Host "本次 MSIX 版本：$($identity.VersionText) -> $($targetVersion.ToString(4))（仅写入临时清单）"
}
try {
  Write-Host "`n==> 构建 $Edition Release MSIX 布局"
  & $script:BuildScript -Edition $Edition -Release -ManifestOverridePath $versionedManifestPath
} finally {
  if (Test-Path -LiteralPath $versionedManifestPath -PathType Leaf) {
    Remove-Item -LiteralPath $versionedManifestPath -Force
  }
}

if ($Edition -eq 'lite') {
  Write-Host "`n==> 验证 Lite 产物边界"
  & $script:LiteBoundaryScript
}

Write-Host "`n==> 使用 CurrentUser\My 证书库中的私钥签名"
Invoke-CheckedCommand $windowsSdkSignTool @('sign', '/fd', 'SHA256', '/sha1', $thumbprint, '/s', 'My', '/v', $script:MsixPath) 'MSIX 签名'

Ensure-MachineTrust $PublicCertificatePath $thumbprint $identity.Publisher
Invoke-CheckedCommand $windowsSdkSignTool @('verify', '/pa', '/all', '/v', $script:MsixPath) 'Windows 信任链验证'
$signature = Get-AuthenticodeSignature -LiteralPath $script:MsixPath
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $thumbprint) {
  throw "MSIX 签名状态或签名者不匹配：$($signature.Status)"
}

$packagedIdentity = Get-PackageIdentityFromMsix $script:MsixPath
if ($packagedIdentity.Name -ne $identity.Name -or $packagedIdentity.Publisher.Trim() -cne $identity.Publisher.Trim() -or $packagedIdentity.Version -ne $targetVersion) {
  throw '打包后的 MSIX Identity 与目标不一致。'
}
Assert-MsixPortraits $script:MsixPath

if ($NoForceApplicationShutdown) {
  Add-AppxPackage -Path $script:MsixPath -ErrorAction Stop
} else {
  Add-AppxPackage -Path $script:MsixPath -ForceApplicationShutdown -ErrorAction Stop
}

$installed = Get-InstalledPackage $identity.Name
if (-not $installed -or [version]$installed.Version -ne $targetVersion) {
  throw "安装后版本不匹配；期望 $targetVersion，实际 $($installed.Version)。"
}
$installedExe = Join-Path $installed.InstallLocation ($script:BinaryName + '.exe')
$packageRootExe = Join-Path (Join-Path $script:ArtifactRoot 'package-root') ($script:BinaryName + '.exe')
if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $packageRootExe -Algorithm SHA256).Hash) {
  throw '已安装 EXE 与本次 package-root EXE 哈希不一致。'
}
Assert-InstalledPortraits $installed.InstallLocation

$mainProcess = $null
if (-not $NoLaunch) {
  $mainProcess = Get-RunningPackageProcesses $installed.InstallLocation | Where-Object { $_.CommandLine -notmatch '--desktop-repair' } | Select-Object -First 1
  if (-not $mainProcess) { Start-Process -FilePath $installedExe }
  $deadline = (Get-Date).AddSeconds(30)
  do {
    $mainProcess = Get-RunningPackageProcesses $installed.InstallLocation | Where-Object { $_.CommandLine -notmatch '--desktop-repair' } | Select-Object -First 1
    if ($mainProcess) { break }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  if (-not $mainProcess) { throw 'MSIX 已安装，但未观察到新版本壁纸进程启动。' }
}

[pscustomobject]@{
  Status = 'published'
  Version = $installed.Version.ToString()
  Edition = $Edition
  Package = $script:MsixPath
  Signature = $signature.Status
  SignerThumbprint = $signature.SignerCertificate.Thumbprint
  InstallLocation = $installed.InstallLocation
  ProcessId = if ($mainProcess) { $mainProcess.ProcessId } else { $null }
  RollbackDirectory = $rollbackDirectory
} | Format-List
