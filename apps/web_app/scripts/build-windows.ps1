param(
    [string]$OpenSslRoot = $env:OPENSSL_DIR,
    [switch]$Release,
    [switch]$Test,
    [string]$TestFilter
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { throw 'This helper is for Windows builds.' }
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
        throw 'No complete x64 OpenSSL development installation was found in the standard locations. Automatic provisioning is not yet available; set OPENSSL_DIR or pass -OpenSslRoot.'
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
$env:LIB = $lib + [IO.Path]::PathSeparator + $env:LIB
$env:PATH = (Join-Path $root 'bin') + [IO.Path]::PathSeparator + $env:PATH
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '1' }
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
Push-Location $repo
try {
    if ($TestFilter -and -not $Test) { throw '-TestFilter requires -Test.' }
    $operation = if ($Test) { 'test' } else { 'build' }
    $cargoArgs = @($operation,'-p','waypoint_web')
    if ($Release) { $cargoArgs += '--release' }
    if ($TestFilter) { $cargoArgs += $TestFilter }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo $operation failed ($LASTEXITCODE)." }
    if ($Test) { Write-Host "Waypoint tests completed using the configured SQLCipher runtime."; return }
    $profile = if ($Release) { 'release' } else { 'debug' }
    $destination = Join-Path $repo "target/$profile/$($dlls[0].Name)"
    if (Test-Path -LiteralPath $destination) {
        if ((Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath $dlls[0].FullName).Hash) { throw "A different DLL already exists at $destination; inspect it before replacing it." }
    } else { Copy-Item -LiteralPath $dlls[0].FullName -Destination $destination }
    $buildReceipt = @{
        profile = $profile
        sqlcipher = $true
        executable_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo "target/$profile/waypoint_web.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
        openssl_dll = $dlls[0].Name
        openssl_sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        cargo_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $buildReceipt | ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $repo "target/$profile/waypoint-build.json")
    Write-Host "Built target/$profile/waypoint_web.exe with SQLCipher; its OpenSSL runtime is beside it."
} finally { Pop-Location }
