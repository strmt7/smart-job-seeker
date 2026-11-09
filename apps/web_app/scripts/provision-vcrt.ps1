param(
    [Parameter(Mandatory=$true)][string]$TargetRoot,
    [Parameter(Mandatory=$true)][string]$InstallerPath
)
# Read-only extraction of Microsoft's pinned redistributable. No installer is run.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
# A PowerShell 7 parent can pass module paths that shadow Windows PowerShell's
# built-ins. Load only this process's OS modules, without changing host settings.
foreach ($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','Microsoft.PowerShell.Security')) {
    Import-Module -Name (Join-Path $PSHOME ('Modules/' + $module + '/' + $module + '.psd1')) -ErrorAction Stop
}
if ($env:OS -ne 'Windows_NT') { throw 'Microsoft runtime extraction requires Windows.' }
$catalog = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../windows-runtime-artifacts.json') -Raw | ConvertFrom-Json
$expectedSize = $catalog.msvc.bytes
$expectedHash = $catalog.msvc.sha256
if ((Get-Item -LiteralPath $InstallerPath).Length -ne $expectedSize -or
    (Get-FileHash -LiteralPath $InstallerPath -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Microsoft runtime package does not match the pinned artifact.' }
$signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation(?:,|$)') { throw 'Microsoft runtime publisher signature is invalid.' }
$root = (Resolve-Path -LiteralPath $TargetRoot).Path
$stage = Join-Path $root ('vcrt-stage-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage | Out-Null
function Expand-PrivateCab([string]$Cab, [string]$Output) {
    New-Item -ItemType Directory -Path $Output | Out-Null
    $log = $Output + '.log'
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $env:SystemRoot 'System32/expand.exe'
    $info.Arguments = '-F:* "' + $Cab + '" "' + $Output + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) { $process.Kill(); $process.WaitForExit(); throw 'Microsoft cabinet extraction timed out.' }
        [IO.File]::WriteAllText($log, ($stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()), [Text.UTF8Encoding]::new($false))
        if ($process.ExitCode -ne 0) { throw "Microsoft cabinet extraction failed ($($process.ExitCode))." }
    } finally { $process.Dispose() }
}
try {
    # Burn's payloads are standard cabinets. The entire publisher-signed EXE
    # has already been size/hash checked; never apply this to arbitrary input.
    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $InstallerPath).Path)
    $cabinets = @()
    for ($offset = 0; $offset -le $bytes.Length - 36; $offset++) {
        if ($bytes[$offset] -ne 77 -or $bytes[$offset+1] -ne 83 -or $bytes[$offset+2] -ne 67 -or $bytes[$offset+3] -ne 70) { continue }
        $length = [BitConverter]::ToUInt32($bytes, $offset+8)
        if ($length -lt 36 -or ([UInt64]$offset + $length) -gt $bytes.Length -or [BitConverter]::ToUInt32($bytes,$offset+4) -ne 0) { continue }
        $cab = Join-Path $stage ('container-' + $cabinets.Count + '.cab')
        $file = [IO.File]::Open($cab,[IO.FileMode]::CreateNew)
        try { $file.Write($bytes,$offset,[int]$length) } finally { $file.Dispose() }
        $cabinets += $cab
        if ($cabinets.Count -gt 2) { throw 'Unexpected Microsoft runtime cabinet inventory.' }
        $offset += [int]$length - 1
    }
    if ($cabinets.Count -ne 2) { throw 'Microsoft runtime cabinets were not found.' }
    $manifestRoot = Join-Path $stage 'manifest'
    $payloadRoot = Join-Path $stage 'payloads'
    Expand-PrivateCab $cabinets[0] $manifestRoot
    Expand-PrivateCab $cabinets[1] $payloadRoot
    # Disable DTD/entity resolution even though the enclosing artifact is pinned.
    $xml = [Xml.XmlDocument]::new()
    $xml.XmlResolver = $null
    $xml.Load((Join-Path $manifestRoot '0'))
    $payloads = @($xml.SelectNodes('//*[local-name()="Payload"]'))
    $minimum = @($payloads | Where-Object { $_.Id -eq 'vcRuntimeMinimum_x64' })
    $cabPayload = @($payloads | Where-Object { $_.FilePath -eq 'packages\vcRuntimeMinimum_amd64\cab1.cab' })
    if ($minimum.Count -ne 1 -or $cabPayload.Count -ne 1 -or $minimum[0].SourcePath -notmatch '^a[0-9]+$' -or $cabPayload[0].SourcePath -notmatch '^a[0-9]+$') { throw 'Microsoft runtime manifest is incompatible.' }
    $unpacked = Join-Path $stage 'files'
    Expand-PrivateCab (Join-Path $payloadRoot $cabPayload[0].SourcePath) $unpacked
    $content = Join-Path $stage 'runtime'
    New-Item -ItemType Directory -Path $content | Out-Null
    $installer = $database = $view = $null
    $inventory = @{}
    try {
        $installer = New-Object -ComObject WindowsInstaller.Installer
        $database = $installer.OpenDatabase((Join-Path $payloadRoot $minimum[0].SourcePath),0)
        $view = $database.OpenView('SELECT `File`, `FileName` FROM `File`')
        $view.Execute()
        while ($row = $view.Fetch()) {
            try {
                $source = $row.StringData(1)
                $name = ($row.StringData(2) -split '\|')[-1]
                if ($source -notmatch '^[A-Za-z0-9_.]+$' -or $name -notmatch '^(?:vcruntime|msvcp|concrt|vccorlib|vcamp|vcomp)140(?:_[a-z0-9_]+)?\.dll$' -or $inventory.ContainsKey($name)) { throw 'Unexpected Microsoft runtime file mapping.' }
                $file = Join-Path $unpacked $source
                if ((Get-Item -LiteralPath $file).Length -gt 4MB) { throw 'Microsoft runtime DLL exceeds its size budget.' }
                Copy-Item -LiteralPath $file -Destination (Join-Path $content $name)
                $inventory[$name] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
                if ($inventory[$name] -ne $catalog.msvc.files.$name) { throw 'Microsoft runtime DLL does not match its pinned digest.' }
            } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($row) }
        }
        $view.Close()
    } finally {
        foreach ($object in @($view,$database,$installer)) { if ($null -ne $object) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) } }
    }
    if ($inventory.Count -ne 12 -or -not $inventory.ContainsKey('msvcp140.dll') -or -not $inventory.ContainsKey('vcruntime140.dll')) { throw 'Microsoft runtime DLL inventory is incomplete.' }
    $receipt = @{ version=$catalog.msvc.version; source_sha256=$expectedHash; files=$inventory }
    [IO.File]::WriteAllText((Join-Path $content 'source.json'),($receipt | ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
    Write-Output $content
} catch { throw "Microsoft runtime extraction failed; private staging retained at $stage. $($_.Exception.Message)" }
