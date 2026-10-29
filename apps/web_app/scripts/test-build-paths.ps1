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
Write-Host 'Build helper path tests passed.'
