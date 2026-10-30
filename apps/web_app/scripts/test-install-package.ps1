$ErrorActionPreference = 'Stop'
$targetPath = Join-Path $PSScriptRoot '../../../target'
New-Item -ItemType Directory -Path $targetPath -Force | Out-Null
$targetRoot = (Resolve-Path -LiteralPath $targetPath).Path
$testRoot = Join-Path $targetRoot ('waypoint-installer-fixture-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $installer = Join-Path $testRoot 'install.ps1'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-package.ps1') -Destination $installer
    $app = Join-Path $testRoot 'app'
    foreach ($name in @('waypoint_web.exe', 'libcrypto-test.dll', 'dependencies/python/python.exe', 'dependencies/libreoffice/program/soffice.com', 'dependencies/libreoffice/LICENSE.html')) {
        $file = Join-Path $app $name
        New-Item -ItemType Directory -Path (Split-Path -Parent $file) -Force | Out-Null
        [IO.File]::WriteAllText($file, "fictional fixture: $name")
    }
    function Hash([string]$name) { (Get-FileHash -LiteralPath (Join-Path $app $name) -Algorithm SHA256).Hash.ToLowerInvariant() }
    $manifest = @{
        executable_sha256 = Hash 'waypoint_web.exe'
        openssl_dll = 'libcrypto-test.dll'
        openssl_sha256 = Hash 'libcrypto-test.dll'
        python_files = @{ 'dependencies/python/python.exe' = (Hash 'dependencies/python/python.exe') }
        libreoffice_files = @{
            'dependencies/libreoffice/program/soffice.com' = (Hash 'dependencies/libreoffice/program/soffice.com')
            'dependencies/libreoffice/LICENSE.html' = (Hash 'dependencies/libreoffice/LICENSE.html')
        }
    }
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    $nested = Join-Path $app 'nested-install'
    $rejected = $false
    try { & $installer -InstallDirectory $nested -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'outside the extracted package') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $nested)) { throw 'An installation inside the package was not rejected before copying.' }
    $rejected = $false
    try { & $installer -InstallDirectory $nested.ToUpperInvariant() -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'outside the extracted package') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $nested)) { throw 'Case-insensitive source containment was not enforced.' }
    $sibling = Join-Path $testRoot 'app2'
    & $installer -InstallDirectory $sibling -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $sibling 'waypoint_web.exe') -PathType Leaf)) { throw 'A sibling with a similar prefix was incorrectly rejected.' }
    $installed = Join-Path $testRoot 'installed'
    & $installer -InstallDirectory $installed -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/libreoffice/program/soffice.com') -PathType Leaf)) { throw 'Bundled converter was not copied.' }
    $licenseHash = $manifest.libreoffice_files['dependencies/libreoffice/LICENSE.html']
    $manifest.libreoffice_files.Remove('dependencies/libreoffice/LICENSE.html')
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    $missingLicense = Join-Path $testRoot 'missing-license'
    $rejected = $false
    try { & $installer -InstallDirectory $missingLicense -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'incomplete LibreOffice') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $missingLicense)) { throw 'Missing license was not rejected before installation.' }
    $manifest.libreoffice_files['dependencies/libreoffice/LICENSE.html'] = $licenseHash
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    [IO.File]::WriteAllText((Join-Path $app 'dependencies/libreoffice/program/soffice.com'), 'damaged')
    $failedDestination = Join-Path $testRoot 'damaged-install'
    $rejected = $false
    try { & $installer -InstallDirectory $failedDestination -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'missing or damaged') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $failedDestination)) { throw 'Damaged converter was not rejected before installation.' }
    Remove-Item -LiteralPath (Join-Path $app 'dependencies/libreoffice/program/soffice.com')
    Remove-Item -LiteralPath (Join-Path $app 'dependencies/libreoffice/LICENSE.html')
    $manifest.libreoffice_files = @{}
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    $withoutOffice = Join-Path $testRoot 'without-office'
    & $installer -InstallDirectory $withoutOffice -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $withoutOffice 'waypoint_web.exe') -PathType Leaf)) { throw 'Package without the optional converter did not install.' }
    Write-Host 'Fictional installer copy, damaged converter and optional converter checks passed.'
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
