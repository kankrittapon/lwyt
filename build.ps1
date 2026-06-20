$ErrorActionPreference = "Stop"

Write-Host "Reading version from Cargo.toml..."
$cargoContent = Get-Content Cargo.toml
$versionLine = $cargoContent | Select-String "^version =" | Select-Object -First 1
if ($null -eq $versionLine) {
    Write-Error "Could not find version in Cargo.toml"
}
$version = $versionLine.ToString().Split([char]34)[1]
Write-Host "Version detected: $version"

Write-Host "Building LYTBokkChoYx in Release mode..."
cargo build --release

# Ensure output directory exists
$targetReleaseDir = "target/release"
if (!(Test-Path $targetReleaseDir)) {
    New-Item -ItemType Directory -Path $targetReleaseDir | Out-Null
}

# 1. Build WiX MSI Installer
Write-Host "Building MSI Installer via WiX Toolset..."
if (Get-Command wix -ErrorAction SilentlyContinue) {
    # Accept EULA for WiX v7
    wix eula accept wix7 2>$null | Out-Null
    wix build installer.wxs -d Version="$version" -o "$targetReleaseDir/LYTBokkChoYx.msi"
    Write-Host "MSI Installer created: $targetReleaseDir/LYTBokkChoYx.msi"
} else {
    Write-Warning "wix command not found. Skipping MSI build."
}

# 2. Build Inno Setup EXE Installer
Write-Host "Building EXE Installer via Inno Setup..."
$isccPath = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if (Test-Path $isccPath) {
    & $isccPath /DMyAppVersion="$version" installer.iss
    Write-Host "EXE Installer created: LYTBokkChoYx_Setup.exe"
} elseif (Get-Command ISCC.exe -ErrorAction SilentlyContinue) {
    & ISCC.exe /DMyAppVersion="$version" installer.iss
    Write-Host "EXE Installer created: LYTBokkChoYx_Setup.exe"
} else {
    Write-Warning "Inno Setup compiler (ISCC.exe) not found. Skipping EXE build."
}

# 3. Verification & Cleanup
Write-Host "Verifying output files..."
if (Test-Path "$targetReleaseDir/lytbokkchoyx.exe") {
    Write-Host "Application Binary: $targetReleaseDir/lytbokkchoyx.exe [OK]"
} else {
    Write-Error "lytbokkchoyx.exe not found!"
}

if (Test-Path "$targetReleaseDir/LYTBokkChoYx.msi") {
    Write-Host "WiX MSI Installer: $targetReleaseDir/LYTBokkChoYx.msi [OK]"
}

if (Test-Path "LYTBokkChoYx_Setup.exe") {
    # Move EXE setup to target/release to match release workflow and clean up root directory
    Move-Item -Path "LYTBokkChoYx_Setup.exe" -Destination "$targetReleaseDir/LYTBokkChoYx_Setup.exe" -Force
    Write-Host "Inno Setup EXE: $targetReleaseDir/LYTBokkChoYx_Setup.exe [OK]"
}

Write-Host "All tasks completed successfully on local machine!"
