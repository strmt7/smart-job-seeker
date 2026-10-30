$ErrorActionPreference = 'Stop'
$target = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../../target')).Path
$source = Join-Path $target 'waypoint-openssl-4.1.0'
if (-not (Test-Path -LiteralPath $source -PathType Container)) { throw 'Provision the pinned OpenSSL cache before this offline fixture.' }
$fixture = Join-Path $target ('waypoint-openssl-fixture-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
    $cache = Join-Path $fixture 'waypoint-openssl-4.1.0'
    New-Item -ItemType Directory -Path $cache | Out-Null
    Copy-Item -LiteralPath (Join-Path $source 'include') -Destination (Join-Path $cache 'include') -Recurse
    foreach ($name in @('lib/libcrypto.lib', 'bin/libcrypto-4-x64.dll', 'LICENSE.txt')) {
        $destination = Join-Path $cache $name
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $source $name) -Destination $destination
    }
    $script = Join-Path $PSScriptRoot 'provision-openssl.ps1'
    if ((& $script -TargetRoot $fixture) -ne $cache) { throw 'Valid cache was not reused.' }
    $header = Join-Path $cache 'include/openssl/ssl.h'
    Add-Content -LiteralPath $header -Value 'fictional corruption'
    $rejected = $false
    try { & $script -TargetRoot $fixture } catch {
        if ($_.Exception.Message -notmatch 'does not match the pinned development files') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'Modified cache was accepted.' }
    $invalidRoot = Join-Path $fixture 'invalid-package'
    New-Item -ItemType Directory -Path $invalidRoot | Out-Null
    $invalidInstaller = Join-Path $fixture 'invalid.msi'
    [IO.File]::WriteAllText($invalidInstaller, 'fictional installer')
    $rejected = $false
    try { & $script -TargetRoot $invalidRoot -InstallerPath $invalidInstaller } catch {
        if ($_.Exception.Message -notmatch 'does not match the pinned package') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath (Join-Path $invalidRoot 'waypoint-openssl-4.1.0'))) { throw 'Invalid installer was accepted.' }
    Write-Host 'OpenSSL cache reuse, modified-header and invalid-installer checks passed.'
} finally {
    if (Test-Path -LiteralPath $fixture) {
        $resolved = (Resolve-Path -LiteralPath $fixture).Path
        if ((Split-Path -Parent $resolved) -ne $target -or -not (Split-Path -Leaf $resolved).StartsWith('waypoint-openssl-fixture-')) { throw 'Refusing to remove an unexpected test directory.' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
