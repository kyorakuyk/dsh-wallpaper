<#
.SYNOPSIS
  Installs the MSIX test package bundled beside this script.

.DESCRIPTION
  This is a deliberately explicit bootstrapper for development/test builds.
  It imports only the matching public certificate into the machine-level
  TrustedPeople store, then delegates package registration to Add-AppxPackage.
  It never handles or creates a PFX/private key and never writes to the root
  certification-authority store.
#>
[CmdletBinding()]
param(
  [switch]$Elevated,
  [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
  Add-Type -AssemblyName System.Windows.Forms
  Add-Type -AssemblyName System.IO.Compression.FileSystem
} catch {
  Write-Error "This installer requires Windows PowerShell with Windows Forms: $($_.Exception.Message)"
  exit 1
}

function Show-Message {
  param(
    [Parameter(Mandatory = $true)][string]$Message,
    [Parameter(Mandatory = $true)][string]$Title,
    [System.Windows.Forms.MessageBoxButtons]$Buttons = [System.Windows.Forms.MessageBoxButtons]::OK,
    [System.Windows.Forms.MessageBoxIcon]$Icon = [System.Windows.Forms.MessageBoxIcon]::Information
  )
  return [System.Windows.Forms.MessageBox]::Show($Message, $Title, $Buttons, $Icon)
}

function Test-Administrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = [Security.Principal.WindowsPrincipal]::new($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Start-Elevated {
  $quotedScript = '"' + $PSCommandPath + '"'
  $arguments = "-NoProfile -ExecutionPolicy Bypass -File $quotedScript -Elevated"
  try {
    $child = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments -Wait -PassThru
    exit $child.ExitCode
  } catch {
    [void](Show-Message -Message "Administrator permission was not granted. No certificate or package was changed.`n`n$($_.Exception.Message)" -Title 'DSH Wallpaper Lite' -Icon Warning)
    exit 1
  }
}

function Get-MsixMetadata {
  param([Parameter(Mandatory = $true)][string]$Path)

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

  try {
    [xml]$manifest = $manifestText
  } catch {
    throw "The MSIX manifest is not valid XML: $($_.Exception.Message)"
  }

  $identity = $manifest.SelectSingleNode("/*[local-name()='Package']/*[local-name()='Identity']")
  if (-not $identity) { throw 'The MSIX manifest has no package identity.' }

  $name = [string]$identity.GetAttribute('Name')
  $publisher = [string]$identity.GetAttribute('Publisher')
  $version = [string]$identity.GetAttribute('Version')
  if ([string]::IsNullOrWhiteSpace($name) -or [string]::IsNullOrWhiteSpace($publisher)) {
    throw 'The MSIX identity is missing Name or Publisher.'
  }

  return [PSCustomObject]@{
    Name = $name
    Publisher = $publisher
    Version = $version
  }
}

function Get-TrustedCertificate {
  param([Parameter(Mandatory = $true)][string]$Thumbprint)

  $store = [Security.Cryptography.X509Certificates.X509Store]::new(
    [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
    [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
  )
  try {
    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
    return @($store.Certificates | Where-Object { $_.Thumbprint -eq $Thumbprint })
  } finally {
    $store.Close()
  }
}

function Add-TestCertificate {
  param([Parameter(Mandatory = $true)][Security.Cryptography.X509Certificates.X509Certificate2]$Certificate)

  $store = [Security.Cryptography.X509Certificates.X509Store]::new(
    [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
    [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
  )
  try {
    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
    $existing = @($store.Certificates | Where-Object { $_.Thumbprint -eq $Certificate.Thumbprint })
    if ($existing.Count -gt 0) {
      return $false
    }
    $store.Add($Certificate)
    return $true
  } finally {
    $store.Close()
  }
}

function Remove-TestCertificateIfUnused {
  param(
    [Parameter(Mandatory = $true)][Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
    [Parameter(Mandatory = $true)][string]$PackageName
  )

  try {
    $installed = @(Get-AppxPackage -Name $PackageName -ErrorAction SilentlyContinue)
    if ($installed.Count -gt 0) {
      return $false
    }

    $store = [Security.Cryptography.X509Certificates.X509Store]::new(
      [Security.Cryptography.X509Certificates.StoreName]::TrustedPeople,
      [Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine
    )
    try {
      $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
      $matches = @($store.Certificates | Where-Object { $_.Thumbprint -eq $Certificate.Thumbprint })
      foreach ($match in $matches) {
        $store.Remove($match)
      }
      return ($matches.Count -gt 0)
    } finally {
      $store.Close()
    }
  } catch {
    return $false
  }
}

if (-not $ValidateOnly -and -not $Elevated -and -not (Test-Administrator)) {
  Start-Elevated
}

$metadata = $null
$certificate = $null
$addedThisRun = $false

try {
  $payloadRoot = $PSScriptRoot
  $msixFiles = @(Get-ChildItem -LiteralPath $payloadRoot -Filter '*.msix' -File)
  $cerFiles = @(Get-ChildItem -LiteralPath $payloadRoot -Filter '*.cer' -File)
  $pfxFiles = @(Get-ChildItem -LiteralPath $payloadRoot -Filter '*.pfx' -File)

  if ($msixFiles.Count -ne 1) { throw "The installer must contain exactly one MSIX file; found $($msixFiles.Count)." }
  if ($cerFiles.Count -ne 1) { throw "The installer must contain exactly one public CER file; found $($cerFiles.Count)." }
  if ($pfxFiles.Count -gt 0) { throw 'The installer must not contain a PFX/private key.' }

  $msixPath = $msixFiles[0].FullName
  $cerPath = $cerFiles[0].FullName
  $metadata = Get-MsixMetadata -Path $msixPath
  $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($cerPath)

  $now = Get-Date
  if ($certificate.NotBefore -gt $now -or $certificate.NotAfter -lt $now) {
    throw "The bundled certificate is outside its validity period ($($certificate.NotBefore) to $($certificate.NotAfter))."
  }

  $publisher = $metadata.Publisher.Trim()
  $certificateSubject = $certificate.Subject.Trim()
  if ($publisher -cne $certificateSubject) {
    throw "Publisher mismatch. The MSIX declares '$publisher', but the CER subject is '$certificateSubject'."
  }

  $signature = Get-AuthenticodeSignature -FilePath $msixPath
  if (-not $signature.SignerCertificate) {
    throw "The MSIX has no readable signing certificate (status: $($signature.Status))."
  }
  $signerThumbprint = $signature.SignerCertificate.Thumbprint
  if ($signerThumbprint -ne $certificate.Thumbprint) {
    throw "The bundled CER does not match the MSIX signer. MSIX: $signerThumbprint; CER: $($certificate.Thumbprint)."
  }

  $integrityStatuses = @('NotSigned', 'HashMismatch', 'Incompatible', 'NotSupported', 'NotSupportedFileSystem')
  if ([string]$signature.Status -in $integrityStatuses) {
    throw "The MSIX signature is not intact (status: $($signature.Status))."
  }

  $hash = (Get-FileHash -LiteralPath $msixPath -Algorithm SHA256).Hash
  $selfSigned = $certificate.Issuer.Trim() -ceq $certificate.Subject.Trim()
  $certificateKind = if ($selfSigned) { 'self-signed test certificate' } else { 'certificate with an issuer chain' }
  if ($ValidateOnly) {
    Write-Output "Validation succeeded: $($msixFiles[0].Name) matches $($cerFiles[0].Name); no certificate or package was changed."
    exit 0
  }
  $prompt = @"
DSH Wallpaper Lite test package

This installer will:
  - add the matching public certificate to Local Machine / Trusted People;
  - install the MSIX package for the current user.

Publisher: $publisher
Certificate: $certificateKind
SHA-1: $($certificate.Thumbprint)
MSIX SHA-256: $hash

This is not a publicly verified production certificate. Continue only if you trust this exact test package. No private key will be installed.
"@
  $choice = Show-Message -Message $prompt -Title 'DSH Wallpaper Lite test installer' -Buttons YesNo -Icon Warning
  if ($choice -ne [System.Windows.Forms.DialogResult]::Yes) {
    exit 0
  }

  $addedThisRun = Add-TestCertificate -Certificate $certificate
  try {
    Add-AppxPackage -Path $msixPath -ErrorAction Stop
  } catch {
    $detail = $_.Exception.Message
    throw "MSIX installation failed. The usual next causes are missing WebView2/VCLibs dependencies or a Windows package policy. Details: $detail"
  }

  [void](Show-Message -Message "Installation completed.`n`nStart the app from the Start menu. The certificate remains installed so future test-package operations can validate this publisher." -Title 'DSH Wallpaper Lite' -Icon Information)
  exit 0
} catch {
  $rollback = ''
  if ($addedThisRun -and $certificate -and $metadata) {
    if (Remove-TestCertificateIfUnused -Certificate $certificate -PackageName $metadata.Name) {
      $rollback = "`n`nThe newly added test certificate was removed because the package is not installed."
    } else {
      $rollback = "`n`nThe test certificate was left in place because its use could not be safely determined."
    }
  }
  [void](Show-Message -Message "Installation stopped.`n`n$($_.Exception.Message)$rollback" -Title 'DSH Wallpaper Lite' -Icon Error)
  exit 1
}
