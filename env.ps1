# Dot-source to use the repo-local Rust toolchain: . .\env.ps1
# .rust is a junction to a sibling repo's toolchain (README: Setup).
$RootPath = $PSScriptRoot

if (-not (Test-Path -LiteralPath "$RootPath\.rust\.cargo\bin\cargo.exe")) {
    throw "No toolchain at .rust. Create the junction first, e.g.: cmd /c mklink /J .rust ..\..\Sabitori\.rust"
}

$env:RUSTUP_HOME = "$RootPath\.rust\.rustup"
$env:CARGO_HOME = "$RootPath\.rust\.cargo"

$BinPath = "$RootPath\.rust\.cargo\bin"
if ($env:PATH -notlike "*$BinPath*") {
    $env:PATH = "$BinPath;" + $env:PATH
}

$SccacheExe = "$BinPath\sccache.exe"
if (Test-Path $SccacheExe) {
    $env:RUSTC_WRAPPER = $SccacheExe
    $env:SCCACHE_DIR = "$RootPath\.rust\.sccache"
} else {
    $env:RUSTC_WRAPPER = ""
}

Write-Host "Local Rust environment loaded." -ForegroundColor Green
Write-Host "RUSTUP_HOME:   $env:RUSTUP_HOME"
Write-Host "CARGO_HOME:    $env:CARGO_HOME"
Write-Host "RUSTC_WRAPPER: $(if ($env:RUSTC_WRAPPER) { $env:RUSTC_WRAPPER } else { 'none' })"
& "$BinPath\rustc.exe" --version
