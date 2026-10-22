# The Windows app, from App/: winsec.dll (Rust), the engine and the signer (Swift, release), the Swift runtime beside
# them, then the window and its installer (Tauri, NSIS, installed for all users into Program Files, where the user's
# own programs can't replace a file). With a code-signing certificate every program and DLL is signed with it before
# the installer is made, and the installer after: the signer then accepts only an engine signed by the same
# certificate.
#
#   pwsh Windows/build.ps1                  # unsigned
#   $env:WINDOWS_PFX = "C:\path\cert.pfx"; $env:WINDOWS_PFX_PASSWORD = "…"; pwsh Windows/build.ps1
#
# Out: Windows/app/src-tauri/target/release/bundle/nsis/ihatecrypto_<version>_x64-setup.exe
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)

$version = (Get-Content Windows/app/src-tauri/tauri.conf.json | ConvertFrom-Json).version
Write-Host "ihatecrypto for Windows $version"

# 1. winsec.dll, and the import library under the name Swift links (-lwinsec).
cargo build --release --locked --manifest-path Windows/native/Cargo.toml
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Copy-Item Windows/native/target/release/winsec.dll.lib Windows/native/target/release/winsec.lib -Force
$env:PATH = "$PWD\Windows\native\target\release;$env:PATH"

# 2. The engine and the signer.
swift build -c release --product ihatecrypto-engine
if ($LASTEXITCODE) { exit $LASTEXITCODE }
swift build -c release --product ihatecrypto-signer
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$out = (swift build -c release --show-bin-path).Trim()

# 3. What the installer puts in bin\: the two programs, winsec.dll and the Swift runtime they load.
$bin = "Windows/app/src-tauri/bin"
Remove-Item $bin -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $bin | Out-Null
Copy-Item "$out/ihatecrypto-engine.exe", "$out/ihatecrypto-signer.exe", Windows/native/target/release/winsec.dll $bin
$swiftCore = (& where.exe swiftCore.dll | Select-Object -First 1)
if (-not $swiftCore) { throw "The Swift runtime (swiftCore.dll) isn't on PATH." }
Copy-Item (Join-Path (Split-Path $swiftCore) "*.dll") $bin
# Resources SwiftPM made for a target (bundles), if any.
Get-ChildItem $out -Directory -Filter "*.resources" | ForEach-Object { Copy-Item $_.FullName $bin -Recurse }

# 4. Signing, when there is a certificate: imported into the user's store, then everything is signed by its thumbprint.
$tauriConfig = @()
if ($env:WINDOWS_PFX) {
    $password = ConvertTo-SecureString $env:WINDOWS_PFX_PASSWORD -AsPlainText -Force
    $cert = Import-PfxCertificate -FilePath $env:WINDOWS_PFX -CertStoreLocation Cert:\CurrentUser\My -Password $password
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
    $files = Get-ChildItem $bin -Recurse -Include *.exe, *.dll | ForEach-Object FullName
    & $signtool.FullName sign /fd sha256 /td sha256 /tr http://timestamp.digicert.com /sha1 $cert.Thumbprint $files
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    # Tauri signs the window's program and the installer with the same certificate.
    $sign = @{ bundle = @{ windows = @{ certificateThumbprint = $cert.Thumbprint; digestAlgorithm = "sha256"; timestampUrl = "http://timestamp.digicert.com" } } }
    $tauriConfig = @("--config", ($sign | ConvertTo-Json -Depth 5 -Compress))
}

# Update archives and their signatures, when the updater's key is given (TAURI_SIGNING_PRIVATE_KEY and its password).
if ($env:TAURI_SIGNING_PRIVATE_KEY) {
    $updates = @{ bundle = @{ createUpdaterArtifacts = $true } } | ConvertTo-Json -Depth 3 -Compress
    $tauriConfig += @("--config", $updates)
}

# 5. The window and the installer.
Push-Location Windows/app
npx --yes @tauri-apps/cli@2.12.4 build @tauriConfig
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Pop-Location
$setup = Get-ChildItem Windows/app/src-tauri/target/release/bundle/nsis/*.exe | Select-Object -First 1
Write-Host "Installer: $($setup.FullName)"
