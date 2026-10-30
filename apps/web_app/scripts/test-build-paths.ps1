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
$staticDefinition = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Get-StaticCrtFlags'
}, $true)
if (-not $staticDefinition) { throw 'Static CRT flag helper is missing.' }
. ([scriptblock]::Create($staticDefinition.Extent.Text))
$emptyFlags = Get-StaticCrtFlags '' ''
if ($emptyFlags.Plain -cne '-C target-feature=+crt-static' -or $emptyFlags.Encoded) { throw 'Empty static CRT flag setup failed.' }
$plainFlags = Get-StaticCrtFlags '' '-C debuginfo=1'
if ($plainFlags.Plain -cne '-C debuginfo=1 -C target-feature=+crt-static') { throw 'Existing Rust flags were not preserved.' }
$encodedFlags = Get-StaticCrtFlags '-Copt-level=1' '-C debuginfo=1'
if ($encodedFlags.Encoded -cne ('-Copt-level=1' + ([char]0x1f).ToString() + '-Ctarget-feature=+crt-static') -or $encodedFlags.Plain -cne '-C debuginfo=1') { throw 'Encoded Rust flags were not preserved.' }
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

    # Exercise the build helper with a fictional OpenSSL layout and a fake
    # Cargo function, including the empty encoded-variable precedence case.
    $fakeOpenSsl = Join-Path $fixture 'fake-openssl'
    New-Item -ItemType Directory -Path (Join-Path $fakeOpenSsl 'include/openssl'), (Join-Path $fakeOpenSsl 'lib'), (Join-Path $fakeOpenSsl 'bin') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $fakeOpenSsl 'include/openssl/ssl.h'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $fakeOpenSsl 'lib/libcrypto.lib'), 'fixture')
    [IO.File]::WriteAllText((Join-Path $fakeOpenSsl 'bin/libcrypto-fixture.dll'), 'fixture')
    function cargo {
        if ($env:WAYPOINT_TEST_FLAG_MODE -eq 'fail') {
            $global:LASTEXITCODE = 9
            return
        } elseif ($env:WAYPOINT_TEST_FLAG_MODE -eq 'plain') {
            if (Test-Path Env:CARGO_ENCODED_RUSTFLAGS) { throw 'Empty encoded flags overrode plain flags.' }
            if ($env:RUSTFLAGS -cne '-C target-feature=+crt-static') { throw 'Static CRT flag missing from plain flags.' }
        } else {
            if ($env:CARGO_ENCODED_RUSTFLAGS -cne ('-Copt-level=1' + ([char]0x1f).ToString() + '-Ctarget-feature=+crt-static')) { throw 'Static CRT flag missing from encoded flags.' }
            if ($env:RUSTFLAGS -cne '-C debuginfo=1') { throw 'Plain flags changed unexpectedly.' }
        }
        $global:LASTEXITCODE = 0
    }
    $env:WAYPOINT_TEST_FLAG_MODE = 'plain'
    $env:RUSTFLAGS = ''
    $env:CARGO_ENCODED_RUSTFLAGS = ''
    & $script -OpenSslRoot $fakeOpenSsl -StaticCrt -Test -TestFilter fixture
    if (-not (Test-Path Env:RUSTFLAGS) -or $env:RUSTFLAGS -ne '' -or -not (Test-Path Env:CARGO_ENCODED_RUSTFLAGS) -or $env:CARGO_ENCODED_RUSTFLAGS -ne '') { throw 'Empty flag variables were not restored.' }
    $env:WAYPOINT_TEST_FLAG_MODE = 'encoded'
    $env:RUSTFLAGS = '-C debuginfo=1'
    $env:CARGO_ENCODED_RUSTFLAGS = '-Copt-level=1'
    & $script -OpenSslRoot $fakeOpenSsl -StaticCrt -Test -TestFilter fixture
    if ($env:RUSTFLAGS -cne '-C debuginfo=1' -or $env:CARGO_ENCODED_RUSTFLAGS -cne '-Copt-level=1') { throw 'Caller flags were not restored.' }
    $env:WAYPOINT_TEST_FLAG_MODE = 'fail'
    $failed = $false
    try { & $script -OpenSslRoot $fakeOpenSsl -StaticCrt -Test -TestFilter fixture } catch {
        if ($_.Exception.Message -notmatch 'Cargo test failed') { throw }
        $failed = $true
    }
    if (-not $failed -or $env:RUSTFLAGS -cne '-C debuginfo=1' -or $env:CARGO_ENCODED_RUSTFLAGS -cne '-Copt-level=1') { throw 'Caller flags were not restored after failure.' }
} finally {
    if (Test-Path -LiteralPath $fixture) {
        $resolved = (Resolve-Path -LiteralPath $fixture).Path
        if ((Split-Path -Parent $resolved) -ne $target -or -not (Split-Path -Leaf $resolved).StartsWith('waypoint-runtime-fixture-')) { throw 'Refusing to remove an unexpected test directory.' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
Write-Host 'Build helper path tests passed.'
