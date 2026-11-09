$ErrorActionPreference = 'Stop'
$sourceInstaller = Join-Path $PSScriptRoot 'install-package.ps1'
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($sourceInstaller, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Installer could not be parsed.' }
$spaceCheck = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-InstallSpace' }, $true)
if (-not $spaceCheck) { throw 'Installer disk-space check is missing.' }
Invoke-Expression $spaceCheck.Extent.Text
Assert-InstallSpace 100 ([UInt64](512MB) + 100)
$rejectedSpace = $false
try { Assert-InstallSpace 100 ([UInt64](512MB) + 99) } catch { $rejectedSpace = $_.Exception.Message -match 'Insufficient destination disk space' }
if (-not $rejectedSpace) { throw 'Insufficient disk space was not rejected.' }
$rejectedOverflow = $false
try { Assert-InstallSpace ([UInt64]::MaxValue) ([UInt64]::MaxValue) } catch { $rejectedOverflow = $true }
if (-not $rejectedOverflow) { throw 'Overflowing package size was not rejected.' }
$targetPath = Join-Path $PSScriptRoot '../../../target'
New-Item -ItemType Directory -Path $targetPath -Force | Out-Null
$targetRoot = (Resolve-Path -LiteralPath $targetPath).Path
$testRoot = Join-Path $targetRoot ('waypoint-installer-fixture-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $shortcutFunction = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WaypointShortcutPath' }, $true)
    if (-not $shortcutFunction) { throw 'Start Menu shortcut path helper is missing.' }
    Invoke-Expression $shortcutFunction.Extent.Text
    $redirectedPrograms = Join-Path $testRoot 'Redirected Programs'
    New-Item -ItemType Directory -Path $redirectedPrograms | Out-Null
    if ((Get-WaypointShortcutPath $redirectedPrograms) -ne (Join-Path $redirectedPrograms 'Smart Jobseeker.lnk')) { throw 'Redirected Programs folder was not used.' }
    $actualPrograms = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if ((Get-WaypointShortcutPath $actualPrograms) -ne (Join-Path $actualPrograms 'Smart Jobseeker.lnk')) { throw 'The current Windows Programs folder was not accepted.' }
    foreach ($badPath in @('', 'relative', (Join-Path $testRoot 'missing'))) {
        $rejected = $false
        try { Get-WaypointShortcutPath $badPath | Out-Null } catch { $rejected = $true }
        if (-not $rejected) { throw 'Unavailable Programs folder was accepted.' }
    }
    $installer = Join-Path $testRoot 'install.ps1'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install-package.ps1') -Destination $installer
    $app = Join-Path $testRoot 'app'
    foreach ($name in @('SmartJobseeker.exe', 'libcrypto-test.dll', 'dependencies/python/python.exe', 'dependencies/libreoffice/program/soffice.com', 'dependencies/libreoffice/LICENSE.html', 'dependencies/chromium/chrome.exe', 'dependencies/chromium/LICENSES.chromium.html', 'dependencies/ollama/ollama.exe', 'dependencies/ollama/LICENSE', 'dependencies/ollama/lib/ollama/LLAMA_CPP_LICENSE', 'dependencies/ollama/lib/ollama/vulkan/ggml-vulkan.dll', 'dependencies/ollama/lib/ollama/rocm_v7_1/ggml-hip.dll')) {
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
    $vcrtFiles = @{}
    $runtimeCatalog = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../windows-runtime-artifacts.json') -Raw | ConvertFrom-Json
    foreach ($entry in $runtimeCatalog.msvc.files.PSObject.Properties) {
        [IO.File]::WriteAllText((Join-Path $app $entry.Name), "fictional runtime: $($entry.Name)")
        $vcrtFiles[$entry.Name] = Hash $entry.Name
    }
    $manifest = @{
        vcrt_files = $vcrtFiles
        executable_sha256 = Hash 'SmartJobseeker.exe'
        openssl_dll = 'libcrypto-test.dll'
        openssl_sha256 = Hash 'libcrypto-test.dll'
        python_files = @{ 'dependencies/python/python.exe' = (Hash 'dependencies/python/python.exe') }
        chromium_files = @{
            'dependencies/chromium/chrome.exe' = (Hash 'dependencies/chromium/chrome.exe')
            'dependencies/chromium/LICENSES.chromium.html' = (Hash 'dependencies/chromium/LICENSES.chromium.html')
        }
        ollama_files = @{
            'dependencies/ollama/ollama.exe' = (Hash 'dependencies/ollama/ollama.exe')
            'dependencies/ollama/LICENSE' = (Hash 'dependencies/ollama/LICENSE')
            'dependencies/ollama/lib/ollama/LLAMA_CPP_LICENSE' = (Hash 'dependencies/ollama/lib/ollama/LLAMA_CPP_LICENSE')
            'dependencies/ollama/lib/ollama/vulkan/ggml-vulkan.dll' = (Hash 'dependencies/ollama/lib/ollama/vulkan/ggml-vulkan.dll')
            'dependencies/ollama/lib/ollama/rocm_v7_1/ggml-hip.dll' = (Hash 'dependencies/ollama/lib/ollama/rocm_v7_1/ggml-hip.dll')
        }
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
        if (-not (Test-Path -LiteralPath (Join-Path $other 'installed/SmartJobseeker.exe'))) { throw 'Safe junction destination did not install.' }
    } finally {
        [IO.Directory]::Delete($otherLink)
    }
    $sibling = Join-Path $testRoot 'app2'
    & $installer -InstallDirectory $sibling -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $sibling 'SmartJobseeker.exe') -PathType Leaf)) { throw 'A sibling with a similar prefix was incorrectly rejected.' }
    $installed = Join-Path $testRoot 'installed'
    & $installer -InstallDirectory $installed -NoShortcut
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'msvcp140.dll') -PathType Leaf)) { throw 'App-local Microsoft runtime was not copied.' }
    [IO.File]::WriteAllText((Join-Path $app 'msvcp140.dll'), 'damaged')
    $rejected = $false
    try { & $installer -InstallDirectory (Join-Path $testRoot 'damaged-vcrt') -NoShortcut } catch { $rejected = $_.Exception.Message -match 'Microsoft runtime file is missing or damaged' }
    if (-not $rejected -or (Test-Path -LiteralPath (Join-Path $testRoot 'damaged-vcrt'))) { throw 'Damaged Microsoft runtime was accepted.' }
    [IO.File]::WriteAllText((Join-Path $app 'msvcp140.dll'), 'fictional runtime: msvcp140.dll')
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/libreoffice/program/soffice.com') -PathType Leaf)) { throw 'Bundled converter was not copied.' }
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/libreoffice/program/hidden.dll') -PathType Leaf) -or
        -not (Test-Path -LiteralPath (Join-Path $installed 'hidden-top.txt') -PathType Leaf)) { throw 'Hidden package files were not copied.' }
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/ollama/ollama.exe') -PathType Leaf)) { throw 'Bundled local runtime was not copied.' }
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'dependencies/chromium/chrome.exe') -PathType Leaf)) { throw 'Bundled browser was not copied.' }
    $extraBrowserFile = Join-Path $app 'dependencies/chromium/unlisted.dll'
    [IO.File]::WriteAllText($extraBrowserFile, 'unlisted fixture')
    $rejected = $false
    try { & $installer -InstallDirectory (Join-Path $testRoot 'unlisted-browser') -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'unlisted file') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'An unlisted browser file was not rejected.' }
    Remove-Item -LiteralPath $extraBrowserFile
    [IO.File]::WriteAllText((Join-Path $app 'dependencies/chromium/chrome.exe'), 'damaged')
    $rejected = $false
    try { & $installer -InstallDirectory (Join-Path $testRoot 'damaged-browser') -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'missing or damaged') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'A damaged browser was not rejected.' }
    [IO.File]::WriteAllText((Join-Path $app 'dependencies/chromium/chrome.exe'), 'fictional fixture: dependencies/chromium/chrome.exe')
    $runtimeLink = Join-Path $app 'dependencies/ollama/linked-directory'
    New-Item -ItemType Junction -Path $runtimeLink -Target $other | Out-Null
    try {
        $linkedDestination = Join-Path $testRoot 'linked-ollama'
        $rejected = $false
        try { & $installer -InstallDirectory $linkedDestination -NoShortcut } catch {
            if ($_.Exception.Message -notmatch 'cannot contain links') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $linkedDestination)) { throw 'An Ollama link was not rejected.' }
    } finally {
        [IO.Directory]::Delete($runtimeLink)
    }
    $unlistedOllama = Join-Path $app 'dependencies/ollama/unlisted.dll'
    [IO.File]::WriteAllText($unlistedOllama, 'unlisted fixture')
    $unlistedDestination = Join-Path $testRoot 'unlisted-ollama'
    $rejected = $false
    try { & $installer -InstallDirectory $unlistedDestination -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'unlisted file') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $unlistedDestination)) { throw 'An unlisted Ollama file was not rejected.' }
    Remove-Item -LiteralPath $unlistedOllama
    $ollamaLicenseHash = $manifest.ollama_files['dependencies/ollama/LICENSE']
    $manifest.ollama_files.Remove('dependencies/ollama/LICENSE')
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    $missingOllama = Join-Path $testRoot 'missing-ollama-license'
    $rejected = $false
    try { & $installer -InstallDirectory $missingOllama -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'incomplete Ollama') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $missingOllama)) { throw 'Missing Ollama license was not rejected.' }
    $manifest.ollama_files['dependencies/ollama/LICENSE'] = $ollamaLicenseHash
    $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    [IO.File]::WriteAllText((Join-Path $app 'dependencies/ollama/ollama.exe'), 'damaged')
    $damagedOllama = Join-Path $testRoot 'damaged-ollama'
    $rejected = $false
    try { & $installer -InstallDirectory $damagedOllama -NoShortcut } catch {
        if ($_.Exception.Message -notmatch 'missing or damaged') { throw }
        $rejected = $true
    }
    if (-not $rejected -or (Test-Path -LiteralPath $damagedOllama)) { throw 'Damaged Ollama runtime was not rejected.' }
    [IO.File]::WriteAllText((Join-Path $app 'dependencies/ollama/ollama.exe'), 'fictional fixture: dependencies/ollama/ollama.exe')
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
    if (-not (Test-Path -LiteralPath (Join-Path $withoutOffice 'SmartJobseeker.exe') -PathType Leaf)) { throw 'Package without the optional converter did not install.' }
    $ollamaSource = [IO.Path]::GetFullPath((Join-Path $app 'dependencies/ollama'))
    $ollamaSaved = [IO.Path]::GetFullPath((Join-Path $testRoot 'saved-ollama'))
    $fixturePrefix = $testRoot.TrimEnd([char[]]'\/') + [IO.Path]::DirectorySeparatorChar
    if (-not $ollamaSource.StartsWith($fixturePrefix, [StringComparison]::OrdinalIgnoreCase) -or -not $ollamaSaved.StartsWith($fixturePrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Runtime fixture paths escaped the test directory.' }
    $savedOllamaInventory = $manifest.ollama_files
    Move-Item -LiteralPath $ollamaSource -Destination $ollamaSaved
    try {
        $manifest.ollama_files = @{}
        $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
        $withoutOllama = Join-Path $testRoot 'without-ollama'
        & $installer -InstallDirectory $withoutOllama -NoShortcut
        if (Test-Path -LiteralPath (Join-Path $withoutOllama 'dependencies/ollama')) { throw 'An omitted runtime was unexpectedly installed.' }
    } finally {
        Move-Item -LiteralPath $ollamaSaved -Destination $ollamaSource
        $manifest.ollama_files = $savedOllamaInventory
        $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $app 'package-manifest.json') -Encoding UTF8
    }
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
