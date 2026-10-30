$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'build-windows.ps1'
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Build helper has a PowerShell syntax error.' }
$definition = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Set-PreferredSearchPath'
}, $true)
if (-not $definition) { throw 'Build helper path function is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))
$preferred = 'C:\SSL\bin'
$first = Set-PreferredSearchPath 'C:\Other;C:\SSL\bin\;C:\More;c:\ssl\BIN' $preferred
if ($first -cne 'C:\SSL\bin;C:\Other;C:\More') { throw 'Path precedence or deduplication failed.' }
$second = Set-PreferredSearchPath $first $preferred
if ($second -cne $first) { throw 'Repeated path setup changed the result.' }
if ((Set-PreferredSearchPath '' $preferred) -cne $preferred) { throw 'Empty path setup failed.' }
$rejected = 0
try { & $script -Test -Clippy } catch {
    if ($_.Exception.Message -notmatch 'cannot be used together') { throw }
    $rejected++
}
try { & $script -TestFilter 'fictional-test' } catch {
    if ($_.Exception.Message -notmatch 'requires -Test') { throw }
    $rejected++
}
try { & $script -Test -Ignored } catch {
    if ($_.Exception.Message -notmatch 'requires -Test and a specific -TestFilter') { throw }
    $rejected++
}
if ($rejected -ne 3) { throw 'Invalid option combinations were not rejected.' }
$runtimeDefinition = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Install-OpenSslRuntime'
}, $true)
if (-not $runtimeDefinition) { throw 'Runtime copy function is missing.' }
. ([scriptblock]::Create($runtimeDefinition.Extent.Text))
$target = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../../target')).Path
$fixture = Join-Path $target ('waypoint-runtime-fixture-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
    $source = Join-Path $fixture 'new.dll'
    $destination = Join-Path $fixture 'libcrypto-test.dll'
    $receipt = Join-Path $fixture 'waypoint-build.json'
    [IO.File]::WriteAllText($source, 'fictional new runtime')
    [IO.File]::WriteAllText($destination, 'fictional old runtime')
    $oldHash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    @{ profile='debug'; openssl_dll='libcrypto-test.dll'; openssl_sha256=$oldHash } | ConvertTo-Json | Set-Content -LiteralPath $receipt -Encoding UTF8
    Install-OpenSslRuntime $source $destination $receipt 'debug'
    if ([IO.File]::ReadAllText($destination) -ne 'fictional new runtime') { throw 'The known runtime was not updated.' }
    $backups = @(Get-ChildItem -LiteralPath (Join-Path $fixture 'waypoint-openssl-backups') -Filter "$oldHash-*-libcrypto-test.dll" -File)
    if ($backups.Count -ne 1) { throw 'Expected one saved runtime backup.' }
    $backup = $backups[0].FullName
    if ([IO.File]::ReadAllText($backup) -ne 'fictional old runtime') { throw 'The old runtime backup is missing.' }
    [IO.File]::WriteAllText($destination, 'unrecognized runtime')
    $refused = $false
    try { Install-OpenSslRuntime $source $destination $receipt 'debug' } catch {
        if ($_.Exception.Message -notmatch 'does not identify') { throw }
        $refused = $true
    }
    if (-not $refused -or [IO.File]::ReadAllText($destination) -ne 'unrecognized runtime') { throw 'An unrecognized runtime was replaced.' }
} finally {
    if (Test-Path -LiteralPath $fixture) {
        $resolved = (Resolve-Path -LiteralPath $fixture).Path
        if ((Split-Path -Parent $resolved) -ne $target -or -not (Split-Path -Leaf $resolved).StartsWith('waypoint-runtime-fixture-')) { throw 'Refusing to remove an unexpected test directory.' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
Write-Host 'Build helper path tests passed.'
