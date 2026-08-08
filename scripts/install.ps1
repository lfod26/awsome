<#
.SYNOPSIS
    Installs the latest `awsome` release for Windows (x86_64).

.DESCRIPTION
    Downloads the latest `awsome` release asset from GitHub, installs it
    to "$HOME\.awsome\awsome.exe" (the config file, awsome_conf.json,
    lives next to the executable), and adds "$HOME\.awsome" to the
    current user's PATH so `awsome` can be run from any shell.

    Only user-scope changes are made (no admin rights required):
      - $HOME\.awsome is created if missing.
      - The downloaded exe is copied there (re-running this script
        upgrades an existing install via -Force).
      - $HOME\.awsome is appended to the User PATH environment variable
        if not already present (idempotent - safe to re-run).
      - The current session's $env:Path is updated too, so `awsome`
        works immediately without opening a new terminal.

.NOTES
    NOTE: GitHub's "latest release" API
    (https://api.github.com/repos/lfod26/awsome/releases/latest) does
    NOT return draft releases. As of writing, this repo's release
    workflow (.github/workflows/release.yml) publishes releases with
    `draft: true`. If no non-draft release has been published yet,
    this script will fail to resolve a "latest" version - pass
    -Version explicitly to work around this, or publish (un-draft) a
    release first.

.PARAMETER Version
    Optional. A specific release tag (e.g. "v0.1.0") to install instead
    of the latest published release. Only usable when running the
    script directly (e.g. ".\install.ps1 -Version v0.1.0"); when piped
    through "irm ... | iex" there's no way to pass PowerShell
    parameters, so set the $env:AWSOME_INSTALL_VERSION environment
    variable instead (see examples below).

.PARAMETER InstallDir
    Optional. Override the install directory. Defaults to
    "$HOME\.awsome". Same caveat as -Version: when using "irm | iex",
    set $env:AWSOME_INSTALL instead.

.EXAMPLE
    irm https://raw.githubusercontent.com/lfod26/awsome/main/scripts/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Version v0.1.0 -InstallDir C:\tools\awsome

.EXAMPLE
    # Overriding version/install dir when installing via "irm | iex"
    # (PowerShell parameters can't be passed through a piped iex, so
    # environment variables are used instead - same pattern Deno's
    # installer uses for DENO_INSTALL):
    $env:AWSOME_INSTALL_VERSION = "v0.1.0"
    $env:AWSOME_INSTALL = "C:\tools\awsome"
    irm https://raw.githubusercontent.com/lfod26/awsome/main/scripts/install.ps1 | iex
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir
)

$ErrorActionPreference = "Stop"

# Support both direct invocation (-Version/-InstallDir params) and the
# "irm | iex" one-liner flow, where params can't be passed through -
# fall back to environment variables in that case.
if (-not $Version -and $env:AWSOME_INSTALL_VERSION) {
    $Version = $env:AWSOME_INSTALL_VERSION
}
if (-not $InstallDir) {
    $InstallDir = if ($env:AWSOME_INSTALL) { $env:AWSOME_INSTALL } else { Join-Path $HOME ".awsome" }
}

$Repo = "lfod26/awsome"
$ExeName = "awsome.exe"

function Get-LatestVersion {
    $uri = "https://api.github.com/repos/$Repo/releases/latest"
    try {
        $release = Invoke-RestMethod -Uri $uri -Headers @{ "User-Agent" = "awsome-installer" }
    } catch {
        throw "Failed to resolve the latest release from $uri. Original error: $_"
    }

    if (-not $release.tag_name) {
        throw "GitHub API response did not contain a tag_name; cannot determine latest version."
    }

    return $release.tag_name
}

# Resolve version and build the download URL. release.yml names assets
# "awsome-$Version-windows-x86_64.exe" where $Version has no leading "v".
$resolvedVersion = if ($Version) { $Version } else { Get-LatestVersion }
$versionNoV = $resolvedVersion.TrimStart("v")
$assetName = "awsome-$versionNoV-windows-x86_64.exe"
$downloadUrl = "https://github.com/$Repo/releases/download/$resolvedVersion/$assetName"

Write-Host "Installing awsome $resolvedVersion..."

# Download to a temp file first so a failed/partial download never
# clobbers an existing working install.
$tempFile = New-TemporaryFile
try {
    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile -UseBasicParsing
    } catch {
        throw "Failed to download $downloadUrl. Verify the version/asset exists. Original error: $_"
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $targetExe = Join-Path $InstallDir $ExeName
    Copy-Item -Path $tempFile -Destination $targetExe -Force
} finally {
    Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
}

# Add the install directory to the User PATH, idempotently. Comparison
# is case-insensitive since Windows paths are case-insensitive.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$alreadyOnPath = $userPath -and (";$userPath;".ToLower() -like "*;$InstallDir;*".ToLower())

if (-not $alreadyOnPath) {
    $newUserPath = if ($userPath) { "$userPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    Write-Host "Added $InstallDir to your User PATH."
} else {
    Write-Host "$InstallDir is already on your User PATH."
}

# Make `awsome` usable immediately in this session too.
if (-not (";$env:Path;".ToLower() -like "*;$InstallDir;*".ToLower())) {
    $env:Path = "$env:Path;$InstallDir"
}

# Verify the install actually runs.
$targetExe = Join-Path $InstallDir $ExeName
try {
    & $targetExe --version | Out-Null
} catch {
    throw "Installed binary at $targetExe failed to run. Original error: $_"
}

Write-Host ""
Write-Host "awsome $resolvedVersion installed to $targetExe" -ForegroundColor Green
Write-Host "Run 'awsome help' to get started."
Write-Host "Note: other already-open terminals won't see the PATH update until restarted."
