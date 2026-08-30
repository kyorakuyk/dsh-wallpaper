<#
.SYNOPSIS
  Builds an IExpress self-extracting test installer containing one MSIX and
  its matching public CER.

.DESCRIPTION
  The generated EXE is a convenience bootstrapper for development/test
  packages. It contains no PFX/private key. The payload is validated before
  IExpress runs: manifest Publisher, MSIX signer certificate, and CER must
  all agree.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$MsixPath,
  [Parameter(Mandatory = $true)][string]$CertificatePath,
  [Parameter(Mandatory = $true)][string]$OutputPath,
  [string]$IExpressPath = (Join-Path $env:windir 'System32\iexpress.exe'),
  [switch]$KeepStaging
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Require-File([string]$Path, [string]$Label) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "$Label was not found: $Path"
  }
  return [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Path).Path)
}

function Get-MsixMetadata {
  param([Parameter(Mandatory = $true)][string]$Path)

  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($Path)
  try {
    $entry = $archive.GetEntry('AppxManifest.xml')
    if (-not $entry) { throw 'The MSIX does not contain AppxManifest.xml.' }
    $stream = $entry.Open()
    try {
      $reader = [IO.StreamReader]::new($stream)
      try {
        $manifestText = $reader.ReadToEnd()
      } finally {
        $reader.Dispose()
      }
    } finally {
      $stream.Dispose()
    }
  } finally {
    $archive.Dispose()
  }

  [xml]$manifest = $manifestText
  $identity = $manifest.SelectSingleNode("/*[local-name()='Package']/*[local-name()='Identity']")
  if (-not $identity) { throw 'The MSIX manifest has no package identity.' }
  return [PSCustomObject]@{
    Name = [string]$identity.GetAttribute('Name')
    Publisher = [string]$identity.GetAttribute('Publisher')
    Version = [string]$identity.GetAttribute('Version')
  }
}

$msix = Require-File $MsixPath 'MSIX package'
$cer = Require-File $CertificatePath 'CER certificate'
$output = [IO.Path]::GetFullPath($OutputPath)
$iexpress = Require-File $IExpressPath 'IExpress'
$sourceScript = Require-File (Join-Path $PSScriptRoot 'install-msix-test.ps1') 'Bootstrapper install script'

if ([IO.Path]::GetExtension($output) -ine '.exe') {
  throw "OutputPath must end in .exe: $output"
}
if (Test-Path -LiteralPath $output -PathType Leaf) {
  throw "Output already exists; remove it explicitly before rebuilding: $output"
}

$metadata = Get-MsixMetadata $msix
$certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($cer)
$signature = Get-AuthenticodeSignature -FilePath $msix
if (-not $signature.SignerCertificate) {
  throw "The MSIX has no readable signing certificate (status: $($signature.Status))."
}
if ($signature.SignerCertificate.Thumbprint -ne $certificate.Thumbprint) {
  throw "MSIX signer and CER do not match. MSIX: $($signature.SignerCertificate.Thumbprint); CER: $($certificate.Thumbprint)."
}
if ($metadata.Publisher.Trim() -cne $certificate.Subject.Trim()) {
  throw "MSIX Publisher and CER subject do not match. MSIX: $($metadata.Publisher); CER: $($certificate.Subject)."
}
$badStatuses = @('NotSigned', 'HashMismatch', 'Incompatible', 'NotSupported', 'NotSupportedFileSystem')
if ([string]$signature.Status -in $badStatuses) {
  throw "The MSIX signature is not intact (status: $($signature.Status))."
}

$outputParent = Split-Path -Parent $output
New-Item -ItemType Directory -Path $outputParent -Force | Out-Null
$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
if (@($output, $tempBase) | Where-Object { $_ -match '\s' }) {
  throw 'IExpress command-line mode cannot safely use paths containing spaces. Move the checkout/output to a path without spaces and retry.'
}
$stageRoot = Join-Path ([IO.Path]::GetTempPath()) "dsh-wallpaper-msix-bootstrapper-$([guid]::NewGuid().ToString('N'))"
$payloadRoot = Join-Path $stageRoot 'payload'
$sedPath = Join-Path $stageRoot 'bootstrapper.sed'

try {
  New-Item -ItemType Directory -Path $payloadRoot -Force | Out-Null
  Copy-Item -LiteralPath $sourceScript -Destination (Join-Path $payloadRoot 'install-msix-test.ps1') -Force
  Copy-Item -LiteralPath $msix -Destination (Join-Path $payloadRoot ([IO.Path]::GetFileName($msix))) -Force
  # Canonicalise the public certificate name. This also lets a developer
  # validate an archived .cer file without accidentally making the runtime
  # installer ignore it because its filename has a diagnostic suffix.
  $stagedCertificateName = 'dsh-wallpaper-test.cer'
  Copy-Item -LiteralPath $cer -Destination (Join-Path $payloadRoot $stagedCertificateName) -Force

  $stagedScript = Join-Path $payloadRoot 'install-msix-test.ps1'
  $stagedMsixName = [IO.Path]::GetFileName($msix)
  $stagedMsix = Join-Path $payloadRoot $stagedMsixName
  $stagedCer = Join-Path $payloadRoot $stagedCertificateName
  $sedContent = @"
[Version]
Class=IEXPRESS
SEDVersion=3

[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=1
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles

[Strings]
InstallPrompt=
DisplayLicense=
FinishMessage=
FriendlyName=DSH Wallpaper Lite test installer
TargetName=$output
AppLaunched=PowerShell.exe -NoProfile -ExecutionPolicy Bypass -File install-msix-test.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
FILE0=install-msix-test.ps1
FILE1=$stagedMsixName
FILE2=$stagedCertificateName

[SourceFiles]
SourceFiles0=$payloadRoot\

[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
"@
  # IExpress' legacy parser is most reliable with an ANSI/ASCII SED file.
  # The CI checkout and the staged filenames are ASCII by design.
  [IO.File]::WriteAllText($sedPath, $sedContent, [Text.Encoding]::ASCII)

  # /N builds from the answer file, /Q suppresses the wizard, and /M keeps
  # any unavoidable build window minimised on Windows versions that ignore Q.
  $process = Start-Process -FilePath $iexpress -ArgumentList @('/N', '/Q', '/M', $sedPath) -Wait -PassThru -NoNewWindow
  if ($process.ExitCode -ne 0) {
    throw "IExpress failed with exit code $($process.ExitCode)."
  }
  if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
    throw "IExpress reported success but did not create the output: $output"
  }

  $outputHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash
  Write-Host "Created test bootstrapper: $output"
  Write-Host "MSIX identity: $($metadata.Name) $($metadata.Version)"
  Write-Host "Publisher: $($metadata.Publisher)"
  Write-Host "Bootstrapper SHA-256: $outputHash"
  if ($KeepStaging) {
    Write-Host "Staging retained: $stageRoot"
  }
} finally {
  if (-not $KeepStaging -and (Test-Path -LiteralPath $stageRoot -PathType Container)) {
    Remove-Item -LiteralPath $stageRoot -Recurse -Force
  }
}
