param(
    [switch]$Release        # Optimised app build (use this to judge conversion speed)
)

# Launch batchWEBP in dev mode with the repo-local Rust toolchain.
. "$PSScriptRoot\env.ps1"

$prevDir = $PWD.Path
Set-Location $PSScriptRoot
try {
    if (-not (Test-Path -LiteralPath "node_modules")) {
        Write-Host "Installing dependencies..." -ForegroundColor Cyan
        npm install
        if ($LASTEXITCODE -ne 0) { Write-Host "npm install failed!" -ForegroundColor Red; exit 1 }
    }
    if ($Release) { npm run tauri dev -- --release } else { npm run tauri dev }
}
finally {
    if (Get-Command sccache -ErrorAction SilentlyContinue) {
        sccache --stop-server 2>&1 | Out-Null
    }
    Set-Location $prevDir
}
