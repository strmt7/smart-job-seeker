param(
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA 'Programs/Waypoint'),
    [switch]$NoShortcut
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Waypoint installation requires Windows.' }
if ($InstallDirectory -notmatch '^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+)') { throw 'Choose an absolute installation path.' }
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
if (Test-Path -LiteralPath $InstallDirectory) { throw 'The installation directory already exists. Choose a new directory; existing files will not be overwritten.' }
$source = Join-Path $PSScriptRoot 'app'
$manifestPath = Join-Path $source 'package-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Package manifest is missing. Extract the complete package and try again.' }
try { $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json } catch { throw 'Package manifest is invalid. Extract the complete package and try again.' }
if ($manifest.openssl_dll -notmatch '^libcrypto[A-Za-z0-9_.-]*\.dll$' -or
    $manifest.openssl_sha256 -notmatch '^[a-fA-F0-9]{64}$' -or
    $manifest.executable_sha256 -notmatch '^[a-fA-F0-9]{64}$') {
    throw 'Package manifest does not contain valid runtime information.'
}
foreach ($entry in @(
    @{ Name = 'waypoint_web.exe'; Hash = $manifest.executable_sha256 },
    @{ Name = $manifest.openssl_dll; Hash = $manifest.openssl_sha256 }
)) {
    $file = Join-Path $source $entry.Name
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Package is missing $($entry.Name). Extract the complete package and try again." }
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Hash) { throw "Package file $($entry.Name) does not match its manifest. Extract the complete package and try again." }
}
$pythonFiles = @($manifest.python_files.PSObject.Properties)
if ($pythonFiles.Count -lt 1 -or $pythonFiles.Count -gt 10000 -or -not $manifest.python_files.'dependencies/python/python.exe') {
    throw 'Package manifest is missing the bundled Python runtime inventory.'
}
foreach ($entry in $pythonFiles) {
    $name = $entry.Name
    if ($name -notmatch '^dependencies/python/[A-Za-z0-9_.+/-]+$' -or $name -match '(^|/)\.\.?(?:/|$)' -or $entry.Value -notmatch '^[a-fA-F0-9]{64}$') {
        throw 'Package manifest contains an invalid Python runtime entry.'
    }
    $file = Join-Path $source $name
    if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Value) {
        throw "Bundled Python file is missing or damaged: $name. Extract the complete package and try again."
    }
}
$shortcut = Join-Path $env:APPDATA 'Microsoft/Windows/Start Menu/Programs/Waypoint.lnk'
if (-not $NoShortcut -and (Test-Path -LiteralPath $shortcut)) { throw 'The Waypoint Start Menu shortcut already exists; it will not be overwritten.' }
New-Item -ItemType Directory -Path $InstallDirectory -ErrorAction Stop | Out-Null
try {
    Copy-Item -Path (Join-Path $source '*') -Destination $InstallDirectory -Recurse -ErrorAction Stop
    if (-not $NoShortcut) {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($shortcut)
        $link.TargetPath = Join-Path $InstallDirectory 'waypoint_web.exe'
        $link.WorkingDirectory = $InstallDirectory
        $link.Description = 'Waypoint local job workspace'
        $link.Save()
    }
    Write-Host "Waypoint copied to $InstallDirectory. Start waypoint_web.exe to open the local workspace."
} catch {
    throw "Installation did not finish. The newly created directory $InstallDirectory may contain partial files; inspect it before retrying. $($_.Exception.Message)"
}
