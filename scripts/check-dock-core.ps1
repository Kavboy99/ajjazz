param([switch]$Embedded)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$cargoPath = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
if (-not (Test-Path -LiteralPath $cargoPath)) { $cargoPath = (Get-Command cargo -ErrorAction Stop).Source }
$savedPath = $env:PATH
$savedFlags = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location -LiteralPath $projectRoot
try {
    $w64 = Join-Path $projectRoot '.tools/w64devkit/bin'
    if (Test-Path -LiteralPath $w64) {
        $env:PATH = "$w64;$env:PATH"
        $env:CARGO_ENCODED_RUSTFLAGS = "-C$([char]31)link-self-contained=yes$([char]31)-C$([char]31)dlltool=$(Join-Path $w64 'dlltool.exe')"
    }
    $manifest = 'firmware/dock-core/Cargo.toml'
    & $cargoPath fmt --manifest-path $manifest -- --check
    if ($LASTEXITCODE -ne 0) { throw 'dock-core formatting failed' }
    & $cargoPath test --locked --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) { throw 'dock-core tests failed' }
    & $cargoPath clippy --locked --manifest-path $manifest --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'dock-core clippy failed' }
    if ($Embedded) {
        $env:CARGO_ENCODED_RUSTFLAGS = $null
        & $cargoPath build --locked --manifest-path $manifest --target thumbv6m-none-eabi --release
        if ($LASTEXITCODE -ne 0) { throw 'dock-core embedded build failed; install thumbv6m-none-eabi first' }
    }
} finally {
    Pop-Location
    $env:PATH = $savedPath
    $env:CARGO_ENCODED_RUSTFLAGS = $savedFlags
}
