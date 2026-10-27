# Common repository build entry point.
# Language-specific builds live under decoders-*/; this wrapper only orchestrates them.
$ErrorActionPreference = "Stop"
$rust = Join-Path $PSScriptRoot "workspace\build.ps1"
if (-not (Test-Path $rust)) {
    Write-Error "workspace/build.ps1 not found: $rust"
    exit 1
}
& $rust
exit $LASTEXITCODE
