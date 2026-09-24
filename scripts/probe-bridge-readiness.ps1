# Phase B readiness probe: capture the Bridge's own diagnosis, never a token.
$ErrorActionPreference = 'Continue'
$base = 'http://127.0.0.1:3080/api/wallpaper/v1'
$status = Invoke-WebRequest -Uri "$base/status" -TimeoutSec 5 -UseBasicParsing
$body = $status.Content | ConvertFrom-Json
[pscustomobject]@{
  httpStatus   = $status.StatusCode
  state        = $body.state
  reasonCode   = $body.reasonCode
  protocol     = $body.protocolVersion
  bridge       = $body.bridgeVersion
  build        = $body.bridgeBuild
  authentication = $body.authentication
  capabilities = ($body.capabilities -join ',')
} | Format-List
Write-Output '--- probes that must never need a token ---'
foreach ($path in @('/status', '/control/presets', '/sessions')) {
  try {
    $r = Invoke-WebRequest -Uri "$base$path" -Method GET -TimeoutSec 5 -UseBasicParsing
    Write-Output ("{0} -> {1}" -f $path, $r.StatusCode)
  } catch {
    Write-Output ("{0} -> {1}" -f $path, $_.Exception.Response.StatusCode.value__)
  }
}
