param(
    [Parameter(Mandatory=$true)][string]$TargetRoot,
    [string]$InstallerPath
)
$ErrorActionPreference = 'Stop'
foreach ($module in @('Microsoft.PowerShell.Management','Microsoft.PowerShell.Utility','Microsoft.PowerShell.Security')) {
    Import-Module -Name (Join-Path $PSHOME ('Modules/' + $module + '/' + $module + '.psd1')) -ErrorAction Stop
}
if ($env:OS -ne 'Windows_NT') { throw 'OpenSSL provisioning requires Windows.' }
$target = (Resolve-Path -LiteralPath $TargetRoot).Path
$cache = Join-Path $target 'waypoint-openssl-4.1.0-2'
$expectedHash = '59803cf501cf4e643a89310a143e964ec9c0432d83a92f3fc585a4fc5a15dff3'
$expectedSize = 60386816
$url = 'https://github.com/openssl/installer/releases/download/publish/4.1.0-2/OpenSSL-x64-hybridCRT-4.1.0.msi'
$expectedTree = '16edd3ef3e7175e0e9c6a0c859df4d0eb4d9a406fa1aa40f166be061646ead16'
function Check-Cache([string]$Directory) {
    $include = Join-Path $Directory 'include'
    if (-not (Test-Path -LiteralPath $include -PathType Container)) { throw "OpenSSL cache is incomplete: $Directory" }
    $names = @(Get-ChildItem -LiteralPath $include -Recurse -File | ForEach-Object { $_.FullName.Substring($Directory.Length + 1).Replace('\', '/') })
    $names += @('lib/libcrypto.lib', 'bin/libcrypto-4-x64.dll', 'LICENSE.txt')
    [Array]::Sort($names, [StringComparer]::Ordinal)
    if ($names.Count -ne 149) { throw "OpenSSL cache has an unexpected file inventory: $Directory" }
    $lines = foreach ($name in $names) {
        $file = Join-Path $Directory $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "OpenSSL cache file is missing: $name" }
        "$name`t$((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant())`n"
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $actual = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join '')))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
    if ($actual -ne $expectedTree) { throw "OpenSSL cache does not match the pinned development files: $Directory" }
}
if (Test-Path -LiteralPath $cache) {
    Check-Cache $cache
    Write-Output $cache
    return
}
$free = (Get-Item -LiteralPath $target).PSDrive.Free
if ($null -ne $free -and $free -lt 1GB) { throw 'At least 1 GiB of free space is required to provision OpenSSL.' }
$stage = Join-Path $target ('waypoint-openssl-stage-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $stage -ErrorAction Stop | Out-Null
$msi = Join-Path $stage 'openssl.msi'
try {
    if ($InstallerPath) {
        Copy-Item -LiteralPath $InstallerPath -Destination $msi -ErrorAction Stop
    } else {
        Write-Host 'Downloading the pinned OpenSSL Windows development package into the build cache.'
        Add-Type -AssemblyName System.Net.Http
        $client = [System.Net.Http.HttpClient]::new()
        $client.Timeout = [TimeSpan]::FromSeconds(180)
        $cancel = [Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(180))
        try {
            $response = $client.GetAsync($url, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancel.Token).GetAwaiter().GetResult()
            try {
                $response.EnsureSuccessStatusCode() | Out-Null
                $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                $out = [IO.File]::Open($msi, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                try {
                    $buffer = New-Object byte[] 65536
                    $size = 0
                    while (($read = $stream.ReadAsync($buffer, 0, $buffer.Length, $cancel.Token).GetAwaiter().GetResult()) -gt 0) {
                        $size += $read
                        if ($size -gt $expectedSize) { throw 'OpenSSL installer exceeds the expected size.' }
                        $out.Write($buffer, 0, $read)
                    }
                } finally { $out.Dispose(); $stream.Dispose() }
            } finally { $response.Dispose() }
        } finally { $cancel.Dispose(); $client.Dispose() }
    }
    if ((Get-Item -LiteralPath $msi).Length -ne $expectedSize -or (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash -ne $expectedHash) { throw 'OpenSSL installer does not match the pinned package.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $msi
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN="?OpenSSL Software Services Czech, s\.r\.o\."?') { throw 'OpenSSL installer signature could not be verified.' }
    $content = Join-Path $stage 'content'
    New-Item -ItemType Directory -Path $content | Out-Null
    $arguments = @('/a', ('"' + $msi + '"'), '/qn', ('TARGETDIR="' + $content + '"'))
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $env:SystemRoot 'System32/msiexec.exe'
    $info.Arguments = $arguments -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(180000)) { throw 'OpenSSL extraction timed out; the installer may still be running.' }
        if ($process.ExitCode -ne 0) { throw "OpenSSL extraction failed ($($process.ExitCode))." }
    } finally { $process.Dispose() }
    Check-Cache $content
    if (Test-Path -LiteralPath $cache) { throw 'Another OpenSSL cache appeared during provisioning; retry using that cache.' }
    [IO.Directory]::Move($content, $cache)
    Check-Cache $cache
    Remove-Item -LiteralPath $msi
    Remove-Item -LiteralPath $stage
    Write-Output $cache
} catch {
    throw "OpenSSL provisioning did not complete; staging retained at $stage. $($_.Exception.Message)"
}
