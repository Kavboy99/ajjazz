param([ValidateSet('build','run','test','check','clippy','fmt','release')][string]$Action='run')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$cargoPath=Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$rustcPath=Join-Path $env:USERPROFILE '.cargo\bin\rustc.exe'
if (-not (Test-Path -LiteralPath $cargoPath)) {$cargoPath=(Get-Command cargo -ErrorAction Stop).Source;$rustcPath=(Get-Command rustc -ErrorAction Stop).Source}
$savedPath=$env:PATH
$savedFlags=$env:CARGO_ENCODED_RUSTFLAGS
$savedLibraries=$env:LIBRARY_PATH
Push-Location -LiteralPath $projectRoot
try {
    $sysroot=& $rustcPath --print sysroot
    $selfContained=Join-Path $sysroot 'lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
    $w64=Join-Path $projectRoot '.tools\w64devkit\bin'
    if ((Test-Path -LiteralPath $selfContained) -and (Test-Path -LiteralPath $w64)) {
        $env:PATH="$w64;$env:PATH"
        $env:CARGO_ENCODED_RUSTFLAGS="-C$([char]31)link-self-contained=yes$([char]31)-C$([char]31)dlltool=$(Join-Path $w64 'dlltool.exe')"
        $extraImports=Join-Path $projectRoot '.tools\gnu-extra'
        New-Item -ItemType Directory -Force -Path $extraImports | Out-Null
        Copy-Item -LiteralPath (Join-Path $projectRoot '.tools\w64devkit\lib\libshlwapi.a') -Destination $extraImports -Force
        $env:LIBRARY_PATH=$extraImports
    }
    switch($Action){
        'run' {& $cargoPath run --locked --bin ajazz-studio}
        'release' {& $cargoPath build --locked --release}
        'fmt' {& $cargoPath fmt --all -- --check}
        'clippy' {& $cargoPath clippy --locked --all-targets -- -D warnings}
        default {& $cargoPath $Action --locked}
    }
    if($LASTEXITCODE -ne 0){throw "cargo $Action failed: $LASTEXITCODE"}
} finally {Pop-Location;$env:PATH=$savedPath;$env:CARGO_ENCODED_RUSTFLAGS=$savedFlags;$env:LIBRARY_PATH=$savedLibraries}
