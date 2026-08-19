<#
.SYNOPSIS
Builds an x64 MSIX *test* package for lock-screen validation.

.DESCRIPTION
This script only creates files under artifacts/msix-test unless an explicit
switch asks it to create/import a test certificate or install a package. It
never modifies the machine certificate stores, AppX registrations, or a
user's lock screen by default.

The common -WhatIf switch deliberately still rebuilds the ignored package
artifacts, so MakeAppx validates the layout from the current inputs. It only
previews the certificate-store and package-install operations.

The package is intentionally an x64 test package. It includes Tauri's runtime
resource layout, the WebView2 loader when dynamically linked, and checks the
Windows runtime prerequisites before it signs anything.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
  [switch]$Release,
  [switch]$CreateTestCertificate,
  [string]$CertificatePath,
  [securestring]$CertificatePassword,
  [switch]$InstallCertificate,
  [switch]$InstallPackage,
  [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Require-Path([string]$Path, [string]$Message) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Message`n$Path" }
}

function Invoke-Checked([string]$FilePath, [string[]]$Arguments, [string]$FailureMessage) {
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) { throw "$FailureMessage (exit code $LASTEXITCODE)" }
}

function Get-WindowsSdkTool([string]$ToolName) {
  $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  if (-not (Test-Path -LiteralPath $sdkRoot -PathType Container)) {
    throw '未找到 Windows SDK；需要 Windows 10/11 SDK 中的 MakeAppx.exe 和 SignTool.exe。'
  }
  $candidate = Get-ChildItem -LiteralPath $sdkRoot -Directory |
    Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName "x64\$ToolName" } |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
    Select-Object -First 1
  if (-not $candidate) { throw "Windows SDK 中缺少 x64\\$ToolName。" }
  return $candidate
}

function Get-PeMachine([string]$Path) {
  Require-Path $Path '无法读取 PE 文件。'
  $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
  try {
    $reader = [IO.BinaryReader]::new($stream)
    try {
      if ($reader.ReadUInt16() -ne 0x5A4D) { throw "不是有效 PE 文件：$Path" }
      $stream.Position = 0x3C
      $peOffset = [int64]$reader.ReadUInt32()
      if ($peOffset -lt 0 -or $peOffset -gt ($stream.Length - 6)) { throw "PE 头偏移无效：$Path" }
      $stream.Position = $peOffset
      if ($reader.ReadUInt32() -ne 0x00004550) { throw "不是有效 PE 文件：$Path" }
      return [int]$reader.ReadUInt16()
    } finally {
      $reader.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Assert-X64Pe([string]$Path, [string]$Label) {
  $machine = Get-PeMachine $Path
  if ($machine -ne 0x8664) {
    throw "$Label 必须是 x64 PE（Machine 应为 0x8664，实际为 0x$('{0:X4}' -f $machine)）：$Path"
  }
}

function Get-WebView2LoaderPath([string]$TauriRoot) {
  # Do not scan Cargo's global cache and take the first x64 DLL: several
  # versions/source registries can coexist there. Ask Cargo which exact
  # webview2-com-sys package this locked workspace resolves, then use the DLL
  # adjacent to that resolved package manifest.
  Push-Location $TauriRoot
  try {
    $metadataRaw = & cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata 无法解析锁定的 WebView2 依赖。' }
  } finally {
    Pop-Location
  }
  try {
    $metadata = ($metadataRaw | Out-String) | ConvertFrom-Json -ErrorAction Stop
  } catch {
    throw "无法解析 Cargo metadata 输出：$($_.Exception.Message)"
  }
  $packages = @($metadata.packages | Where-Object { $_.name -eq 'webview2-com-sys' })
  if ($packages.Count -ne 1) {
    throw "锁定依赖中应恰好有一个 webview2-com-sys，实际为 $($packages.Count) 个；拒绝从不确定的 Cargo 缓存选择 WebView2Loader.dll。"
  }
  $manifestPath = [string]$packages[0].manifest_path
  if ([string]::IsNullOrWhiteSpace($manifestPath) -or -not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw 'Cargo metadata 未提供可读取的 webview2-com-sys 清单路径。'
  }
  $packageRoot = Split-Path -Parent $manifestPath
  $candidate = Join-Path $packageRoot 'x64\WebView2Loader.dll'
  Require-Path $candidate '锁定的 webview2-com-sys 包缺少 x64 WebView2Loader.dll。'
  Assert-X64Pe $candidate 'WebView2Loader.dll'
  return $candidate
}

function Assert-PackagedRuntimeDependencies {
  # The MSVC-built executable imports the Desktop VCLibs framework package.
  # Do not bundle system DLLs: require the official framework instead.
  $vclibs = Get-AppxPackage -Name 'Microsoft.VCLibs.140.00.UWPDesktop' -ErrorAction SilentlyContinue |
    Where-Object { $_.Architecture -eq 'X64' } |
    Select-Object -First 1
  if (-not $vclibs) {
    throw '未检测到 x64 Microsoft.VCLibs.140.00.UWPDesktop。请从 Microsoft Store / App Installer 安装官方 Visual C++ UWP Desktop Runtime 后重试。'
  }

  # Tauri/Wry uses the Evergreen WebView2 Runtime. Its loader is packaged
  # below, but the runtime itself is supplied by Edge WebView2.
  $webViewRuntime = Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'
  if (-not (Test-Path -LiteralPath $webViewRuntime -PathType Container)) {
    throw '未检测到 Microsoft Edge WebView2 Evergreen Runtime。请安装官方 WebView2 Runtime 后重试。'
  }
}

function Remove-TestCertificate([System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate) {
  if (-not $Certificate) { return }
  $store = [System.Security.Cryptography.X509Certificates.X509Store]::new('My', 'CurrentUser')
  try {
    $store.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
    $match = $store.Certificates | Where-Object { $_.Thumbprint -eq $Certificate.Thumbprint } | Select-Object -First 1
    if ($match) { $store.Remove($match) }
  } finally {
    $store.Close()
  }
}

function Remove-NewTestCertificateArtifacts([string]$PfxPath, [string]$CerPath) {
  # These paths are checked for absence before this invocation creates either
  # file. Never call this helper for a caller-supplied certificate.
  foreach ($path in @($PfxPath, $CerPath)) {
    if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) {
      Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    }
  }
}

function Copy-Tree([string]$Source, [string]$Destination, [string]$Label) {
  if (-not (Test-Path -LiteralPath $Source -PathType Container)) { throw "缺少$Label：$Source" }
  # Keep packaging meaningful under the script's -WhatIf mode.  The only
  # expected changes are in the ignored artifacts directory; certificate and
  # AppX registration actions remain behind this script's explicit
  # ShouldProcess calls below.
  New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force -WhatIf:$false | Out-Null
  Copy-Item -LiteralPath $Source -Destination $Destination -Recurse -Force -WhatIf:$false
}

function Get-PlainPassword([securestring]$Value) {
  $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Value)
  try { return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer) }
  finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer) }
}

function Get-MsixManifestMetadata([string]$ManifestPath) {
  try {
    [xml]$manifest = Get-Content -LiteralPath $ManifestPath -Raw
  } catch {
    throw "无法读取 MSIX 清单：$($_.Exception.Message)"
  }
  $identity = $manifest.Package.Identity
  $publisher = [string]$identity.Publisher
  if ([string]::IsNullOrWhiteSpace($publisher)) {
    throw 'MSIX 清单缺少 Identity Publisher。'
  }
  $architecture = [string]$identity.ProcessorArchitecture
  if ($architecture -cne 'x64') {
    throw "锁屏测试 MSIX 仅支持 x64；清单 ProcessorArchitecture 必须为 x64，实际为：$architecture"
  }
  $desktopFamilies = @($manifest.Package.Dependencies.TargetDeviceFamily | Where-Object { $_.Name -eq 'Windows.Desktop' })
  if ($desktopFamilies.Count -ne 1) {
    throw "MSIX 清单必须恰好包含一个 Windows.Desktop TargetDeviceFamily，实际为 $($desktopFamilies.Count) 个。"
  }
  $minimumVersion = [string]$desktopFamilies[0].MinVersion
  try {
    $minimumVersionValue = [version]$minimumVersion
  } catch {
    throw "MSIX 清单 Windows.Desktop MinVersion 无效：$minimumVersion"
  }
  if ($minimumVersionValue -lt [version]'10.0.22000.0') {
    throw "锁屏测试包的支持边界是 Windows 11；Windows.Desktop MinVersion 不能低于 10.0.22000.0，实际为：$minimumVersion"
  }
  return [PSCustomObject]@{
    Publisher = $publisher.Trim()
    ProcessorArchitecture = $architecture
    MinVersion = $minimumVersion
  }
}

function Assert-CertificateMatchesManifestPublisher(
  [string]$Path,
  [securestring]$Password,
  [string]$ManifestPublisher
) {
  if (-not (Get-Command Get-PfxData -ErrorAction SilentlyContinue)) {
    throw '无法读取 PFX 以验证 MSIX Publisher；需要 Windows 的 PKI PowerShell 模块。'
  }
  try {
    $pfx = Get-PfxData -FilePath $Path -Password $Password -ErrorAction Stop
  } catch {
    throw "无法读取签名 PFX：$($_.Exception.Message)"
  }
  $signingCertificates = @($pfx.EndEntityCertificates | Where-Object { $_.HasPrivateKey })
  if ($signingCertificates.Count -ne 1) {
    throw '签名 PFX 必须恰好包含一个带私钥的终端证书，才能安全验证 MSIX Publisher。'
  }
  $subject = [string]$signingCertificates[0].Subject
  if ([string]::IsNullOrWhiteSpace($subject) -or $subject -cne $ManifestPublisher) {
    throw "签名证书 Subject 与 MSIX 清单 Publisher 不一致。清单：$ManifestPublisher；证书：$subject。请使用 Subject 完全匹配的证书，或为该发布者建立独立的 MSIX 清单。"
  }
}

function Assert-RepositoryChild([string]$Parent, [string]$Child, [string]$Label) {
  $parentFull = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  $childFull = [IO.Path]::GetFullPath($Child)
  # Windows PowerShell 5.1 lacks [IO.Path]::GetRelativePath. URI
  # relativization is available there and preserves the same containment
  # decision without relying on an unsafe string-prefix check.
  $parentUri = [Uri]::new(($parentFull + [IO.Path]::DirectorySeparatorChar))
  $childUri = [Uri]::new($childFull)
  $relative = [Uri]::UnescapeDataString($parentUri.MakeRelativeUri($childUri).ToString()).Replace('/', [IO.Path]::DirectorySeparatorChar)
  if ([IO.Path]::IsPathRooted($relative) -or $relative -eq '..' -or $relative.StartsWith("..$([IO.Path]::DirectorySeparatorChar)") -or $relative.StartsWith("..$([IO.Path]::AltDirectorySeparatorChar)")) {
    throw "拒绝清理不在 $Label 中的目录：$childFull"
  }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$tauriRoot = Join-Path $projectRoot 'wallpaper\src-tauri'
$artifactRoot = Join-Path $projectRoot 'artifacts\msix-test'
$cargoTargetRoot = Join-Path $artifactRoot 'cargo-target'
$packageRoot = Join-Path $artifactRoot 'package-root'
$manifestTemplate = Join-Path $projectRoot 'packaging\msix\AppxManifest.xml'
$rustTarget = 'x86_64-pc-windows-msvc'
$makeAppx = Get-WindowsSdkTool 'makeappx.exe'
$signTool = Get-WindowsSdkTool 'signtool.exe'

Require-Path $manifestTemplate '缺少 MSIX 清单模板。'
Require-Path (Join-Path $tauriRoot 'icons\icon.png') '缺少应用图标。'
Require-Path (Join-Path $projectRoot 'wallpaper\public\personas\wake-frames\variant-anima\sleep.png') '缺少锁屏睡眠图。'
$manifest = Get-MsixManifestMetadata $manifestTemplate
$manifestPublisher = $manifest.Publisher

if ($InstallCertificate -and -not $CreateTestCertificate -and -not $CertificatePath) {
  throw '-InstallCertificate 需要 -CreateTestCertificate 或 -CertificatePath。'
}
if ($CreateTestCertificate -and $CertificatePath) {
  throw '-CreateTestCertificate 与 -CertificatePath 不能同时使用。'
}
if (($CreateTestCertificate -or $CertificatePath) -and -not $CertificatePassword -and -not $WhatIfPreference) {
  $CertificatePassword = Read-Host '输入 PFX 密码（仅本地测试使用）' -AsSecureString
}

if (-not $SkipBuild) {
  Push-Location $projectRoot
  try {
    Invoke-Checked 'pnpm' @('-C', 'wallpaper', 'build') '前端构建失败。'
    Push-Location $tauriRoot
    try {
      $cargoArgs = @('build', '--locked', '--target', $rustTarget)
      if ($Release) { $cargoArgs += '--release' }
      # Keep this test build independent of wallpaper/src-tauri/target. A
      # running development instance can hold files in the default target
      # directory, while this isolated output is safe to package and remove.
      $previousCargoTargetDir = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'Process')
      try {
        $env:CARGO_TARGET_DIR = $cargoTargetRoot
        Invoke-Checked 'cargo' $cargoArgs 'Rust 构建失败。'
      } finally {
        if ($null -eq $previousCargoTargetDir) {
          Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue -WhatIf:$false
        } else {
          $env:CARGO_TARGET_DIR = $previousCargoTargetDir
        }
      }
    } finally { Pop-Location }
  } finally { Pop-Location }
}

$profile = if ($Release) { 'release' } else { 'debug' }
$binaryRoot = Join-Path (Join-Path $cargoTargetRoot $rustTarget) $profile
$exe = Join-Path $binaryRoot 'dsh-wallpaper.exe'
$frontendDist = Join-Path $projectRoot 'wallpaper\dist'
Require-Path $exe "未找到隔离 x64 MSIX 构建的 dsh-wallpaper.exe；请移除 -SkipBuild，以 cargo --locked --target $rustTarget 构建 artifacts/msix-test/cargo-target 中相应 profile 的程序。"
Require-Path (Join-Path $frontendDist 'index.html') '未找到前端 dist/index.html；请移除 -SkipBuild 或先完成前端构建。'
Assert-X64Pe $exe 'dsh-wallpaper.exe'

# MakeAppx expects a clean directory. The only recursive deletion is a fully
# resolved, literal child under this repository's artifacts root.
if (Test-Path -LiteralPath $packageRoot) {
  $resolvedPackageRoot = (Resolve-Path -LiteralPath $packageRoot).Path
  Assert-RepositoryChild $artifactRoot $resolvedPackageRoot 'artifacts/msix-test'
  Remove-Item -LiteralPath $resolvedPackageRoot -Recurse -Force -WhatIf:$false
}
New-Item -ItemType Directory -Path $packageRoot -Force -WhatIf:$false | Out-Null
New-Item -ItemType Directory -Path (Join-Path $packageRoot 'Assets') -Force -WhatIf:$false | Out-Null
New-Item -ItemType Directory -Path (Join-Path $packageRoot '_up_\public\personas\wake-frames\variant-anima') -Force -WhatIf:$false | Out-Null

Copy-Item -LiteralPath $exe -Destination (Join-Path $packageRoot 'dsh-wallpaper.exe') -Force -WhatIf:$false
Copy-Item -LiteralPath $manifestTemplate -Destination (Join-Path $packageRoot 'AppxManifest.xml') -Force -WhatIf:$false
Copy-Item -LiteralPath (Join-Path $tauriRoot 'icons\icon.png') -Destination (Join-Path $packageRoot 'Assets\StoreLogo.png') -Force -WhatIf:$false
Copy-Item -LiteralPath (Join-Path $tauriRoot 'icons\icon.png') -Destination (Join-Path $packageRoot 'Assets\Square150x150Logo.png') -Force -WhatIf:$false
Copy-Item -LiteralPath (Join-Path $tauriRoot 'icons\icon.png') -Destination (Join-Path $packageRoot 'Assets\Square44x44Logo.png') -Force -WhatIf:$false
Copy-Tree $frontendDist (Join-Path $packageRoot 'dist') '前端资源'
Copy-Item -LiteralPath (Join-Path $projectRoot 'wallpaper\public\personas\wake-frames\variant-anima\sleep.png') -Destination (Join-Path $packageRoot '_up_\public\personas\wake-frames\variant-anima\sleep.png') -Force -WhatIf:$false

# `webview2-com-sys` uses static linking on MSVC today, but preserve this
# loader alongside the EXE as a packaging invariant. If the dependency
# switches to dynamic linkage in a future update, the MSIX remains runnable.
$webView2Loader = Get-WebView2LoaderPath $tauriRoot
Copy-Item -LiteralPath $webView2Loader -Destination (Join-Path $packageRoot 'WebView2Loader.dll') -Force -WhatIf:$false

$requiredPackageFiles = @(
  'AppxManifest.xml', 'dsh-wallpaper.exe', 'WebView2Loader.dll', 'dist\index.html',
  '_up_\public\personas\wake-frames\variant-anima\sleep.png',
  'Assets\StoreLogo.png', 'Assets\Square150x150Logo.png', 'Assets\Square44x44Logo.png'
)
foreach ($relative in $requiredPackageFiles) {
  Require-Path (Join-Path $packageRoot $relative) "MSIX 打包布局不完整，缺少：$relative"
}
$packagedSleep = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $packageRoot '_up_\public\personas\wake-frames\variant-anima\sleep.png')
$sourceSleep = Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $projectRoot 'wallpaper\public\personas\wake-frames\variant-anima\sleep.png')
if ($packagedSleep.Hash -ne $sourceSleep.Hash) { throw 'MSIX 包中的锁屏睡眠图与正式素材不一致。' }
# Layout packaging is intentionally possible on a build machine that does not
# have the runtime installed. The prerequisites are only required before an
# explicit local installation attempt.
if ($InstallPackage -and -not $WhatIfPreference) { Assert-PackagedRuntimeDependencies }

New-Item -ItemType Directory -Path $artifactRoot -Force -WhatIf:$false | Out-Null
$msix = Join-Path $artifactRoot 'dsh-wallpaper-lockscreen-test.msix'
if (Test-Path -LiteralPath $msix) { Remove-Item -LiteralPath $msix -Force -WhatIf:$false }
Invoke-Checked $makeAppx @('pack', '/o', '/h', 'SHA256', '/d', $packageRoot, '/p', $msix) 'MakeAppx 打包失败。'

$newTestCertificate = $null
if ($CreateTestCertificate) {
  $subject = 'CN=DSH Wallpaper Test'
  $CertificatePath = Join-Path $artifactRoot 'dsh-wallpaper-test.pfx'
  $cerPath = Join-Path $artifactRoot 'dsh-wallpaper-test.cer'
  if (-not $PSCmdlet.ShouldProcess('Cert:\CurrentUser\My', '创建并导出 DSH Wallpaper MSIX 测试签名证书')) {
    Write-Host 'WhatIf：未创建测试证书，也未导出 PFX/CER；将只完成未签名 MSIX 布局打包。'
    $CertificatePath = $null
    $cerPath = $null
  } else {
    if (Test-Path -LiteralPath $CertificatePath) {
      throw "测试 PFX 已存在，拒绝覆盖：$CertificatePath。请先移除旧文件，或使用 -CertificatePath 复用它。"
    }
    if (Test-Path -LiteralPath $cerPath) {
      throw "测试 CER 已存在，拒绝覆盖：$cerPath。请先移除旧文件，或使用 -CertificatePath 复用它。"
    }
    $certificate = New-SelfSignedCertificate `
      -Type Custom `
      -KeyUsage DigitalSignature `
      -KeyExportPolicy Exportable `
      -CertStoreLocation 'Cert:\CurrentUser\My' `
      -TextExtension @(
        '2.5.29.37={text}1.3.6.1.5.5.7.3.3',
        '2.5.29.19={text}'
      ) `
      -Subject $subject `
      -FriendlyName 'DSH Wallpaper MSIX Test'
    $newTestCertificate = $certificate
    try {
      Export-PfxCertificate -Cert $certificate -FilePath $CertificatePath -Password $CertificatePassword | Out-Null
      Export-Certificate -Cert $certificate -FilePath $cerPath | Out-Null
    } catch {
      # It was created by this invocation only. Remove every artifact that
      # may have been written before the failure, plus the private key entry.
      Remove-NewTestCertificateArtifacts $CertificatePath $cerPath
      Remove-TestCertificate $certificate
      throw
    }
  }
} else {
  $CertificatePath = if ($CertificatePath) { [IO.Path]::GetFullPath($CertificatePath) } else { $null }
  $cerPath = if ($CertificatePath) { [IO.Path]::ChangeExtension($CertificatePath, '.cer') } else { $null }
}

if ($CertificatePath -and $WhatIfPreference) {
  Write-Host 'WhatIf：未使用证书签名 MSIX；将保留未签名布局包。'
  $CertificatePath = $null
  $cerPath = $null
} elseif ($CertificatePath) {
  Require-Path $CertificatePath '未找到签名证书。'
  if (-not $CertificatePassword) { throw '签名 PFX 需要 -CertificatePassword。' }
  # Windows requires the signed package's manifest publisher to match the
  # certificate subject.  Fail before SignTool so a seemingly successful
  # signature cannot lead to an uninstallable test package.
  Assert-CertificateMatchesManifestPublisher $CertificatePath $CertificatePassword $manifestPublisher
  $plainPassword = Get-PlainPassword $CertificatePassword
  try {
    Invoke-Checked $signTool @('sign', '/fd', 'SHA256', '/f', $CertificatePath, '/p', $plainPassword, $msix) 'SignTool 签名失败。'
  } catch {
    # External certificates are never touched. A certificate created by this
    # invocation has not yet been trusted or used to install a package, so it
    # is safe to remove only when the signature itself could not be written.
    if ($newTestCertificate) {
      Remove-NewTestCertificateArtifacts $CertificatePath $cerPath
      Remove-TestCertificate $newTestCertificate
    }
    throw
  } finally {
    $plainPassword = $null
  }
  Write-Host 'MSIX 已签名。尚未验证 Windows 信任链；只有显式导入/安装路径才会运行 SignTool /pa。'
} else {
  Write-Warning '未提供签名证书：已生成未签名 MSIX，仅可用于检查打包布局，不能安装。使用 -CreateTestCertificate 或 -CertificatePath 生成可安装测试包。'
}

if ($InstallCertificate) {
  if ($WhatIfPreference -and -not $cerPath) {
    Write-Host 'WhatIf：未导入测试证书到 CurrentUser\TrustedPeople。'
  } else {
    Require-Path $cerPath '未找到对应 .cer 公钥证书，无法导入当前用户信任存储。'
    if ($PSCmdlet.ShouldProcess('Cert:\CurrentUser\TrustedPeople', "导入测试证书 $cerPath")) {
      Import-Certificate -FilePath $cerPath -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
    }
  }
}

$trustedSignatureVerified = $false
if (($InstallCertificate -or $InstallPackage) -and $CertificatePath) {
  if ($WhatIfPreference) {
    Write-Host 'WhatIf：实际导入/安装时将在此处执行 SignTool /pa 受信任签名验证。'
  } else {
    # `/pa` verifies Windows' Authenticode policy and trust chain. It is
    # intentionally delayed until after the caller explicitly asks to import
    # trust (or proves an already trusted external certificate before install).
    # A freshly created self-signed PFX is expected to fail this gate before it
    # is trusted; that is not a SignTool signing failure.
    Invoke-Checked $signTool @('verify', '/pa', '/all', '/v', $msix) 'MSIX 受信任签名验证失败。证书可能尚未受 Windows 信任；软件包未安装。'
    $trustedSignatureVerified = $true
  }
}

if ($InstallPackage) {
  if ($WhatIfPreference -and -not $CertificatePath) {
    Write-Host 'WhatIf：未安装 MSIX（测试证书未创建，因此包保持未签名）。'
  } else {
    if (-not $CertificatePath) { throw '-InstallPackage 需要已签名的 MSIX。请使用 -CreateTestCertificate 或 -CertificatePath。' }
    if (-not $WhatIfPreference -and -not $trustedSignatureVerified) { throw '安装前必须通过 MSIX 受信任签名验证。' }
    if ($PSCmdlet.ShouldProcess($msix, '安装当前用户 MSIX 测试包')) {
      Add-AppxPackage -Path $msix
    }
  }
}

Write-Host "已生成 MSIX 测试包：$msix"
if ($CertificatePath) { Write-Host "签名证书：$CertificatePath" }
if (-not $InstallPackage) { Write-Host '未安装软件包（默认无副作用）。验证并安装时，请显式传入 -InstallCertificate -InstallPackage。' }
