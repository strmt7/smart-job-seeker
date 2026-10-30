param(
    [string]$OpenSslRoot = $env:OPENSSL_DIR,
    [string]$OpenSslInstaller,
    [switch]$Release,
    [switch]$Test,
    [switch]$Clippy,
    [switch]$Ignored,
    [string]$TestFilter
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { throw 'This helper is for Windows builds.' }
if ($Test -and $Clippy) { throw '-Test and -Clippy cannot be used together.' }
if ($TestFilter -and -not $Test) { throw '-TestFilter requires -Test.' }
if ($Ignored -and (-not $Test -or -not $TestFilter)) { throw '-Ignored requires -Test and a specific -TestFilter.' }
if ($OpenSslInstaller -and $PSBoundParameters.ContainsKey('OpenSslRoot') -and $OpenSslRoot) { throw 'Choose either -OpenSslRoot or -OpenSslInstaller.' }
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
if ($OpenSslInstaller) {
    New-Item -ItemType Directory -Path (Join-Path $repo 'target') -Force | Out-Null
    $OpenSslRoot = & (Join-Path $PSScriptRoot 'provision-openssl.ps1') -TargetRoot (Join-Path $repo 'target') -InstallerPath $OpenSslInstaller
}
if (-not $OpenSslRoot) {
    # The official Windows runner and common developer installers use different
    # directories. Only select a complete x64 development installation.
    $candidates = @('OpenSSL-Win64', 'OpenSSL') | ForEach-Object { Join-Path $env:ProgramFiles $_ }
    $OpenSslRoot = $candidates | Where-Object {
        $candidate = $_
        (Test-Path -LiteralPath (Join-Path $candidate 'include/openssl/ssl.h') -PathType Leaf) -and
        (@('lib/VC/x64/MD', 'lib/x64', 'lib') | Where-Object {
            Test-Path -LiteralPath (Join-Path (Join-Path $candidate $_) 'libcrypto.lib') -PathType Leaf
        }).Count -gt 0 -and
        (Test-Path -LiteralPath (Join-Path $candidate 'bin') -PathType Container) -and
        @(Get-ChildItem -LiteralPath (Join-Path $candidate 'bin') -Filter 'libcrypto*.dll' -File).Count -eq 1
    } | Select-Object -First 1
    if (-not $OpenSslRoot) {
        New-Item -ItemType Directory -Path (Join-Path $repo 'target') -Force | Out-Null
        $OpenSslRoot = & (Join-Path $PSScriptRoot 'provision-openssl.ps1') -TargetRoot (Join-Path $repo 'target')
    }
}
if (-not (Test-Path -LiteralPath $OpenSslRoot -PathType Container)) { throw "OpenSSL directory does not exist: $OpenSslRoot" }
$root = (Resolve-Path -LiteralPath $OpenSslRoot).Path
$include = Join-Path $root 'include'
if (-not (Test-Path -LiteralPath (Join-Path $include 'openssl/ssl.h'))) { throw 'OpenSSL headers were not found. Supply -OpenSslRoot with an OpenSSL developer installation.' }
$lib = @('lib/VC/x64/MD','lib/x64','lib') | ForEach-Object { Join-Path $root $_ } | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'libcrypto.lib') } | Select-Object -First 1
if (-not $lib) { throw 'The x64 OpenSSL import library libcrypto.lib was not found.' }
$dlls = @(Get-ChildItem -LiteralPath (Join-Path $root 'bin') -Filter 'libcrypto*.dll' -File)
if ($dlls.Count -ne 1) { throw 'Expected exactly one libcrypto DLL in OpenSSL bin. Choose a single compatible developer installation.' }
$env:OPENSSL_DIR = $root
$env:OPENSSL_INCLUDE_DIR = $include
$env:OPENSSL_LIB_DIR = $lib
function Set-PreferredSearchPath([string]$Current, [string]$Preferred) {
    $separator = [IO.Path]::PathSeparator
    $key = $Preferred.TrimEnd([char[]]'\/')
    $remaining = @($Current -split [regex]::Escape([string]$separator) | Where-Object {
        -not [string]::Equals($_.TrimEnd([char[]]'\/'), $key, [StringComparison]::OrdinalIgnoreCase)
    })
    if (-not $Current) { return $Preferred }
    return (@($Preferred) + $remaining) -join $separator
}
function Install-OpenSslRuntime([string]$Source, [string]$Destination, [string]$ReceiptPath, [string]$Profile) {
    $newHash = (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash.ToLowerInvariant()
    if (-not (Test-Path -LiteralPath $Destination)) {
        Copy-Item -LiteralPath $Source -Destination $Destination -ErrorAction Stop
        return
    }
    $oldHash = (Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($oldHash -eq $newHash) { return }
    if (-not (Test-Path -LiteralPath $ReceiptPath -PathType Leaf)) { throw "A different DLL exists at $Destination without a build receipt; inspect it before replacing it." }
    try { $previous = Get-Content -LiteralPath $ReceiptPath -Raw | ConvertFrom-Json } catch { throw 'The previous build receipt is invalid; the existing DLL was not replaced.' }
    if ($previous.profile -ne $Profile -or $previous.openssl_dll -ne (Split-Path -Leaf $Destination) -or $previous.openssl_sha256 -ne $oldHash) {
        throw "The previous build receipt does not identify the existing DLL at $Destination; it was not replaced."
    }
    $backupDir = Join-Path (Split-Path -Parent $Destination) 'waypoint-openssl-backups'
    New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
    $backup = Join-Path $backupDir ($oldHash + '-' + [guid]::NewGuid() + '-' + (Split-Path -Leaf $Destination))
    $temporary = $Destination + '.new-' + [guid]::NewGuid()
    [IO.File]::Copy($Source, $temporary, $false)
    if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash -ne $newHash) { throw "The staged OpenSSL runtime is damaged; it remains at $temporary." }
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ne $oldHash) { throw "The existing OpenSSL runtime changed during the build; the staged file remains at $temporary." }
    [IO.File]::Replace($temporary, $Destination, $backup)
}
$env:LIB = Set-PreferredSearchPath $env:LIB $lib
$env:PATH = Set-PreferredSearchPath $env:PATH (Join-Path $root 'bin')
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '1' }
Push-Location $repo
try {
    $operation = if ($Test) { 'test' } elseif ($Clippy) { 'clippy' } else { 'build' }
    $cargoArgs = @($operation,'-p','waypoint_web')
    if ($Release) { $cargoArgs += '--release' }
    if ($TestFilter) { $cargoArgs += $TestFilter }
    if ($Clippy) { $cargoArgs += @('--all-targets', '--', '-D', 'warnings') }
    if ($Ignored) { $cargoArgs += @('--', '--ignored', '--test-threads=1') }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo $operation failed ($LASTEXITCODE)." }
    if ($Test) { Write-Host "Waypoint tests completed using the configured SQLCipher runtime."; return }
    if ($Clippy) { Write-Host "Waypoint lint completed using the configured SQLCipher runtime."; return }
    $profile = if ($Release) { 'release' } else { 'debug' }
    $destination = Join-Path $repo "target/$profile/$($dlls[0].Name)"
    Install-OpenSslRuntime $dlls[0].FullName $destination (Join-Path $repo "target/$profile/waypoint-build.json") $profile
    $buildReceipt = @{
        profile = $profile
        sqlcipher = $true
        openssl_root = $root
        executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo "target/$profile/waypoint_web.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
        openssl_dll = $dlls[0].Name
        openssl_sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        cargo_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $buildReceipt | ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $repo "target/$profile/waypoint-build.json")
    Write-Host "Built target/$profile/waypoint_web.exe with SQLCipher; its OpenSSL runtime is beside it."
} finally { Pop-Location }
