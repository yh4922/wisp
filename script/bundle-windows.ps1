<#
.SYNOPSIS
    Package already-built Wisp release binaries into a Windows installer and a portable zip.

.DESCRIPTION
    Used both by .github/workflows/release.yml and for local packaging.
    Compatible with Windows PowerShell 5.1 and PowerShell 7.

    Inputs (from `cargo build -p zed -p cli --release`):
        <TargetDir>\wisp.exe
        <TargetDir>\cli.exe
    Outputs (in <OutputDir>):
        Wisp-<version>-<arch>-setup.exe      Inno Setup installer
        Wisp-<version>-<arch>-portable.zip   portable build (top-level folder "Wisp\")

    The script does NOT run cargo unless -Build is given.

.EXAMPLE
    .\script\bundle-windows.ps1
.EXAMPLE
    .\script\bundle-windows.ps1 -Build -SkipInstaller
#>
[CmdletBinding()]
Param(
    # Directory that contains wisp.exe and cli.exe. Defaults to $env:CARGO_TARGET_DIR\release or target\release.
    [string]$TargetDir,
    # Where the installer and zip are written. Defaults to target\dist.
    [string]$OutputDir,
    # Version string. Defaults to the version in crates\zed\Cargo.toml.
    [string]$Version,
    [ValidateSet('x86_64')]
    [string]$Architecture = 'x86_64',
    # Run `cargo build -p zed -p cli --release` first (local convenience).
    [switch]$Build,
    [switch]$SkipInstaller,
    [switch]$SkipPortable
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Invoke-Native {
    param([string]$File, [string[]]$Arguments)
    Write-Host ">> $File $($Arguments -join ' ')"
    & $File @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$File failed with exit code $LASTEXITCODE" }
}

function Get-WispVersion {
    $inPackage = $false
    foreach ($line in Get-Content (Join-Path $Root 'crates\zed\Cargo.toml')) {
        if ($line -match '^\[package\]') { $inPackage = $true; continue }
        if ($line -match '^\[') { $inPackage = $false }
        if ($inPackage -and $line -match '^version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    throw 'Could not read version from crates\zed\Cargo.toml'
}

function Find-Iscc {
    $cmd = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    # Prefer Inno Setup 6 (what wisp.iss is tested with); 7 is accepted as a fallback.
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe"
    )
    foreach ($c in $candidates) { if ($c -and (Test-Path $c)) { return $c } }
    throw 'Inno Setup 6 (ISCC.exe) not found. Install it (e.g. `choco install innosetup` or `winget install JRSoftware.InnoSetup`).'
}

# ConPTY: conpty.dll + OpenConsole.exe give the integrated terminal a newer console host.
# Optional: if neither the download nor the copies placed by crates/zed/build.rs are
# available, Wisp falls back to the system ConPTY.
function Add-ConPty {
    param([string]$Stage)
    $url = 'https://github.com/microsoft/terminal/releases/download/v1.25.1912.0/Microsoft.Windows.Console.ConPTY.1.25.260710002-preview.nupkg'
    $tmp = Join-Path $Root 'target\wisp-bundle-cache'
    $nupkg = Join-Path $tmp 'conpty.nupkg.zip'
    $extract = Join-Path $tmp 'conpty'
    New-Item -ItemType Directory -Force $tmp | Out-Null
    try {
        if (-not (Test-Path (Join-Path $extract 'runtimes'))) {
            Invoke-WebRequest -Uri $url -OutFile $nupkg -UseBasicParsing
            if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
            Expand-Archive -Path $nupkg -DestinationPath $extract -Force
        }
        New-Item -ItemType Directory -Force (Join-Path $Stage 'x64'), (Join-Path $Stage 'arm64') | Out-Null
        Copy-Item (Join-Path $extract 'runtimes\win-x64\native\conpty.dll') (Join-Path $Stage 'conpty.dll') -Force
        Copy-Item (Join-Path $extract 'build\native\runtimes\x64\OpenConsole.exe') (Join-Path $Stage 'x64\OpenConsole.exe') -Force
        Copy-Item (Join-Path $extract 'build\native\runtimes\arm64\OpenConsole.exe') (Join-Path $Stage 'arm64\OpenConsole.exe') -Force
        return
    } catch {
        Write-Warning "ConPTY download failed ($($_.Exception.Message)); falling back to local copies."
    }
    $dll = Join-Path $TargetDir 'conpty.dll'
    if (Test-Path $dll) { Copy-Item $dll (Join-Path $Stage 'conpty.dll') -Force }
    $oc = Join-Path $TargetDir 'OpenConsole.exe'
    if (-not (Test-Path $oc)) { $oc = Join-Path $Root 'crates\zed\resources\windows\bin\x64\OpenConsole.exe' }
    if (Test-Path $oc) {
        New-Item -ItemType Directory -Force (Join-Path $Stage 'x64') | Out-Null
        Copy-Item $oc (Join-Path $Stage 'x64\OpenConsole.exe') -Force
    }
}

# AMD GPU Services: optional, loaded at runtime by gpui_windows if present.
# Keep the version in sync with crates/gpui_windows/src/directx_renderer.rs.
function Add-AmdAgs {
    param([string]$Stage)
    $tmp = Join-Path $Root 'target\wisp-bundle-cache'
    $zip = Join-Path $tmp 'AGS_SDK_v6.3.0.zip'
    $dll = Join-Path $tmp 'AGS_SDK-6.3.0\ags_lib\lib\amd_ags_x64.dll'
    try {
        if (-not (Test-Path $dll)) {
            New-Item -ItemType Directory -Force $tmp | Out-Null
            Invoke-WebRequest -Uri 'https://codeload.github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/zip/refs/tags/v6.3.0' -OutFile $zip -UseBasicParsing
            Expand-Archive -Path $zip -DestinationPath $tmp -Force
        }
        Copy-Item $dll (Join-Path $Stage 'amd_ags_x64.dll') -Force
    } catch {
        Write-Warning "AMD AGS download failed ($($_.Exception.Message)); skipping amd_ags_x64.dll."
    }
}

# ---------------------------------------------------------------------------

if (-not $Version) { $Version = Get-WispVersion }
if (-not $TargetDir) {
    $cargoTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
    $TargetDir = Join-Path $cargoTarget 'release'
}
if (-not $OutputDir) { $OutputDir = Join-Path $Root 'target\dist' }
New-Item -ItemType Directory -Force $OutputDir | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path

$channel = (Get-Content (Join-Path $Root 'crates\zed\RELEASE_CHANNEL') -Raw).Trim()
if ($channel -ne 'stable') {
    Write-Warning "crates\zed\RELEASE_CHANNEL is '$channel'; installer metadata (AppId, mutex, names) assumes 'stable'."
}

Write-Host "Wisp $Version ($channel, $Architecture)"
Write-Host "  binaries: $TargetDir"
Write-Host "  output:   $OutputDir"

if ($Build) {
    Push-Location $Root
    try {
        $env:ZED_BUNDLE = 'true'
        $env:RELEASE_VERSION = $Version
        Invoke-Native 'cargo' @('build', '--release', '-p', 'zed', '-p', 'cli')
    } finally { Pop-Location }
}

$wispExe = Join-Path $TargetDir 'wisp.exe'
$cliExe = Join-Path $TargetDir 'cli.exe'
foreach ($f in @($wispExe, $cliExe)) {
    if (-not (Test-Path $f)) { throw "Missing $f - build first: cargo build -p zed -p cli --release" }
}

# --- Stage ----------------------------------------------------------------
$resources = Join-Path $Root 'crates\zed\resources\windows'
$stage = Join-Path $Root "target\wisp-bundle\$Architecture"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage 'bin') | Out-Null

Copy-Item $wispExe (Join-Path $stage 'Wisp.exe') -Force
Copy-Item $cliExe (Join-Path $stage 'bin\wisp.exe') -Force
Copy-Item (Join-Path $resources 'wisp.sh') (Join-Path $stage 'bin\wisp') -Force
Add-ConPty -Stage $stage
Add-AmdAgs -Stage $stage

# --- Portable zip -----------------------------------------------------------
if (-not $SkipPortable) {
    $zipPath = Join-Path $OutputDir "Wisp-$Version-$Architecture-portable.zip"
    $zipRoot = Join-Path $Root "target\wisp-bundle\portable-$Architecture"
    if (Test-Path $zipRoot) { Remove-Item $zipRoot -Recurse -Force }
    $appDir = Join-Path $zipRoot 'Wisp'
    New-Item -ItemType Directory -Force $appDir | Out-Null
    Copy-Item (Join-Path $stage '*') $appDir -Recurse -Force
    # Portable build keeps cargo's lower-case name.
    Move-Item (Join-Path $appDir 'Wisp.exe') (Join-Path $appDir 'wisp.exe.tmp') -Force
    Move-Item (Join-Path $appDir 'wisp.exe.tmp') (Join-Path $appDir 'wisp.exe') -Force
    Copy-Item (Join-Path $Root 'LICENSE-GPL') $appDir -Force
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    # Build the zip entry by entry: ZipFile.CreateFromDirectory on Windows PowerShell 5.1
    # (.NET Framework) writes '\' separators, which break unzip tools outside Windows.
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::Open($zipPath, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $prefixLength = (Resolve-Path $zipRoot).Path.TrimEnd('\').Length + 1
        foreach ($file in Get-ChildItem $zipRoot -Recurse -File) {
            $entryName = $file.FullName.Substring($prefixLength).Replace('\', '/')
            [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $entryName, [System.IO.Compression.CompressionLevel]::Optimal)
        }
    } finally {
        $zip.Dispose()
    }
    Write-Host "Portable zip: $zipPath"
}

# --- Installer ----------------------------------------------------------------
if (-not $SkipInstaller) {
    # The .iss reads icons and messages relative to ResourcesDir.
    Copy-Item (Join-Path $resources 'wisp.iss') $stage -Force
    Copy-Item (Join-Path $resources 'app-icon.ico') $stage -Force
    Copy-Item (Join-Path $resources 'messages') $stage -Recurse -Force

    $iscc = Find-Iscc
    $isccArgs = @(
        '/Q',
        "/DVersion=$Version",
        "/DArch=$Architecture",
        "/DAppSetupName=Wisp-$Version-$Architecture-setup",
        "/DResourcesDir=$stage",
        "/DSourceDir=$Root",
        "/DOutputDir=$OutputDir",
        (Join-Path $stage 'wisp.iss')
    )
    Invoke-Native $iscc $isccArgs
    Write-Host "Installer: $(Join-Path $OutputDir "Wisp-$Version-$Architecture-setup.exe")"
}

Write-Host 'Bundle finished.'
