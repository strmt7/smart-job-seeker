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
    $hiddenRuntime = Join-Path $app 'dependencies/libreoffice/program/hidden.dll'
    [IO.File]::WriteAllText($hiddenRuntime, 'fictional hidden runtime')
    (Get-Item -LiteralPath $hiddenRuntime -Force).Attributes = [IO.FileAttributes]::Hidden
    $hiddenTop = Join-Path $app 'hidden-top.txt'
    [IO.File]::WriteAllText($hiddenTop, 'fictional hidden package file')
    (Get-Item -LiteralPath $hiddenTop -Force).Attributes = [IO.FileAttributes]::Hidden
    function Hash([string]$name) { (Get-FileHash -LiteralPath (Join-Path $app $name) -Algorithm SHA256).Hash.ToLowerInvariant() }
    $manifest = @{
        executable_sha256 = Hash 'waypoint_web.exe'
        openssl_dll = 'libcrypto-test.dll'
        openssl_sha256 = Hash 'libcrypto-test.dll'
        python_files = @{ 'dependencies/python/python.exe' = (Hash 'dependencies/python/python.exe') }
        libreoffice_files = @{
            'dependencies/libreoffice/program/soffice.com' = (Hash 'dependencies/libreoffice/program/soffice.com')
            'dependencies/libreoffice/program/hidden.dll' = (Hash 'dependencies/libreoffice/program/hidden.dll')
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
    # The junction itself is removed without recursion before fixture cleanup.
    $junction = Join-Path $testRoot 'source-alias'
    New-Item -ItemType Junction -Path $junction -Target $app | Out-Null
    try {
        $aliasedNested = Join-Path $junction 'nested-through-junction'
        $rejected = $false
        try { & $installer -InstallDirectory $aliasedNested -NoShortcut } catch {
            if ($_.Exception.Message -notmatch 'outside the extracted package') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $aliasedNested)) { throw 'Junction into the package was not rejected.' }
    } finally {
        [IO.Directory]::Delete($junction)
    }
    $aliasPackage = Join-Path $testRoot 'alias-package'
    New-Item -ItemType Directory -Path $aliasPackage | Out-Null
    Copy-Item -LiteralPath $installer -Destination (Join-Path $aliasPackage 'install.ps1')
    $sourceLink = Join-Path $aliasPackage 'app'
    New-Item -ItemType Junction -Path $sourceLink -Target $app | Out-Null
    try {
        $rejected = $false
        try { & (Join-Path $aliasPackage 'install.ps1') -InstallDirectory (Join-Path $app 'nested-real-source') -NoShortcut } catch {
            if ($_.Exception.Message -notmatch 'outside the extracted package') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath (Join-Path $app 'nested-real-source'))) { throw 'Aliased source containment was not enforced.' }
    } finally {
        [IO.Directory]::Delete($sourceLink)
    }
    $other = Join-Path $testRoot 'other-target'
    New-Item -ItemType Directory -Path $other | Out-Null
    $otherLink = Join-Path $testRoot 'other-alias'
    New-Item -ItemType Junction -Path $otherLink -Target $other | Out-Null
    try {
        & $installer -InstallDirectory (Join-Path $otherLink 'installed') -NoShortcut
        if (-not (Test-Path -LiteralPath (Join-Path $other 'installed/waypoint_web.exe'))) { throw 'Safe junction destination did not install.' }
    } finally {
        [IO.Directory]::Delete($otherLink)
    }
    $sibling = Join-Path $testRoot 'app2'
    & $installer -InstallDirectory $sibling -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $sibling 'waypoint_web.exe') -PathType Leaf)) { throw 'A sibling with a similar prefix was incorrectly rejected.' }
    $installed = Join-Path $testRoot 'installed'
    & $installer -InstallDirectory $installed -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/libreoffice/program/soffice.com') -PathType Leaf)) { throw 'Bundled converter was not copied.' }
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/libreoffice/program/hidden.dll') -PathType Leaf) -or
        -not (Test-Path -LiteralPath (Join-Path $installed 'hidden-top.txt') -PathType Leaf)) { throw 'Hidden package files were not copied.' }
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
    # Simulate corruption after copying, before the installer reports success.
    function Copy-Item {
        [CmdletBinding()]
        param([string]$LiteralPath, [string]$Destination, [switch]$Recurse, [switch]$Force)
        Microsoft.PowerShell.Management\Copy-Item @PSBoundParameters
        if ([IO.Path]::GetFileName($LiteralPath) -eq 'dependencies') {
            [IO.File]::WriteAllText((Join-Path $Destination 'dependencies/python/python.exe'), 'fictional copy damage')
        }
    }
    try {
        $copyDamage = Join-Path $testRoot 'copy-damaged'
        $rejected = $false
        try { & $installer -InstallDirectory $copyDamage -NoShortcut } catch {
            if ($_.Exception.Message -notmatch 'Installed dependency file is missing or damaged') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Installed dependency corruption was not detected.' }
    } finally {
        Remove-Item Function:\Copy-Item
    }
    Write-Host 'Fictional installer copy, damaged converter and optional converter checks passed.'
} finally {
    if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}
