# Lumen installer for Windows. In PowerShell:
#
#   irm https://raw.githubusercontent.com/OnurByte/Lumen/main/install.ps1 | iex
#
# Installs for the current user only into %LOCALAPPDATA%\Programs\Lumen after checking the
# release archive's SHA-256, adds it to the user PATH and creates a Start menu shortcut.
# Options (environment variables): LUMEN_VERSION, LUMEN_BIN_DIR,
# LUMEN_ARCHIVE=C:\path\lumen-windows-x86_64.zip (with .sha256 beside it).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$Repo = if ($env:LUMEN_REPO) { $env:LUMEN_REPO } else { 'OnurByte/Lumen' }
$Version = if ($env:LUMEN_VERSION) { $env:LUMEN_VERSION } else { 'latest' }
$InstallDir = if ($env:LUMEN_BIN_DIR) { $env:LUMEN_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\Lumen' }
$Data = Join-Path $env:USERPROFILE '.local\share\lumen'
$Binaries = 'lumend.exe', 'lumen-studio.exe', 'lumen-live.exe'

function Fail($Message) {
    Write-Host "Lumen install stopped: $Message" -ForegroundColor Red
    throw $Message
}

if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    Fail 'Only 64-bit x86 Windows is supported for now.'
}

$Asset = 'lumen-windows-x86_64.zip'
if ($Version -eq 'latest') {
    $Base = "https://github.com/$Repo/releases/latest/download"
} else {
    $Base = "https://github.com/$Repo/releases/download/$Version"
}
$Temp = Join-Path ([IO.Path]::GetTempPath()) ("lumen-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null

try {
    Write-Host 'Downloading Lumen (Windows, x86_64)…'
    $Zip = Join-Path $Temp $Asset
    if ($env:LUMEN_ARCHIVE) {
        Copy-Item $env:LUMEN_ARCHIVE $Zip
        Copy-Item "$($env:LUMEN_ARCHIVE).sha256" "$Zip.sha256"
    } else {
        try {
            Invoke-WebRequest "$Base/$Asset" -OutFile $Zip -UseBasicParsing
            Invoke-WebRequest "$Base/$Asset.sha256" -OutFile "$Zip.sha256" -UseBasicParsing
        } catch {
            Fail "Could not download $Asset. There may be no release yet: https://github.com/$Repo/releases"
        }
    }
    $Expected = ((Get-Content "$Zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
    if (-not $Expected -or $Expected -ne $Actual) {
        Fail 'SHA-256 mismatch; the download is damaged or altered. Nothing was installed.'
    }

    Expand-Archive $Zip -DestinationPath $Temp -Force
    $Package = Join-Path $Temp 'lumen'
    foreach ($Binary in $Binaries) {
        if (-not (Test-Path (Join-Path $Package $Binary))) { Fail "$Binary is missing from the archive" }
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Previous = Join-Path $InstallDir '.lumen-previous'
    foreach ($Binary in $Binaries) {
        $Target = Join-Path $InstallDir $Binary
        if (Test-Path $Target) {
            # Keep the replaced version for rollback; a running .exe can only be renamed aside.
            New-Item -ItemType Directory -Force -Path $Previous | Out-Null
            Copy-Item $Target (Join-Path $Previous $Binary) -Force
            $Aside = Join-Path $InstallDir ".$Binary.old"
            Remove-Item $Aside -Force -ErrorAction SilentlyContinue
            Rename-Item $Target $Aside
        }
        Copy-Item (Join-Path $Package $Binary) $Target -Force
    }

    # Shipped shows are refreshed; shows you created are left alone.
    New-Item -ItemType Directory -Force -Path (Join-Path $Data 'packs') | Out-Null
    Copy-Item (Join-Path $Package 'packs\*') (Join-Path $Data 'packs') -Recurse -Force

    $Current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $Current) { $Current = '' }
    if (($Current -split ';') -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', ($Current.TrimEnd(';') + ";$InstallDir").TrimStart(';'), 'User')
    }

    $StartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    $Shell = New-Object -ComObject WScript.Shell
    $Shortcut = $Shell.CreateShortcut((Join-Path $StartMenu 'Lumen.lnk'))
    $Shortcut.TargetPath = Join-Path $InstallDir 'lumen-studio.exe'
    $Shortcut.WorkingDirectory = $InstallDir
    $Shortcut.Save()

    if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
        Write-Host 'FFmpeg not found. To stream, run: winget install Gyan.FFmpeg'
    }
    Write-Host ''
    Write-Host "Installed Lumen in $InstallDir"
    Write-Host 'Open Lumen from the Start menu.'
} finally {
    Remove-Item $Temp -Recurse -Force -ErrorAction SilentlyContinue
}
