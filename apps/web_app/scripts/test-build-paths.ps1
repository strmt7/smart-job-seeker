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
Write-Host 'Build helper path tests passed.'
