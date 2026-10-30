param(
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA 'Programs/Waypoint'),
    [switch]$NoShortcut
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Waypoint installation requires Windows.' }
if ($InstallDirectory -notmatch '^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+)') { throw 'Choose an absolute installation path.' }
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
if (Test-Path -LiteralPath $InstallDirectory) { throw 'The installation directory already exists. Choose a new directory; existing files will not be overwritten.' }
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'app'))
# Copying the package into its own tree can recursively copy the destination.
# Compare full path components rather than a raw prefix (app2 is not app).
$sourcePrefix = $source.TrimEnd([char[]]'\/') + [IO.Path]::DirectorySeparatorChar
if ($InstallDirectory.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Choose an installation directory outside the extracted package app folder.'
}
$manifestPath = Join-Path $source 'package-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Package manifest is missing. Extract the complete package and try again.' }
# Resolve the existing source and destination ancestors through junctions and
# symlinks before comparing them. Never follow a link by deleting or moving it.
if (-not ('WaypointInstallerPath' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public static class WaypointInstallerPath {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint GetFinalPathNameByHandle(SafeFileHandle handle, StringBuilder path, uint size, uint flags);
    public static string ResolveDirectory(string path) {
        using (var handle = CreateFile(path, 0, 7, IntPtr.Zero, 3, 0x02000000, IntPtr.Zero)) {
            if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            var value = new StringBuilder(32768);
            uint length = GetFinalPathNameByHandle(handle, value, (uint)value.Capacity, 0);
            if (length == 0) throw new Win32Exception(Marshal.GetLastWin32Error());
            if (length >= value.Capacity) throw new InvalidOperationException("Resolved path is too long.");
            string result = value.ToString();
            if (result.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) return @"\\" + result.Substring(8);
            if (result.StartsWith(@"\\?\", StringComparison.OrdinalIgnoreCase)) return result.Substring(4);
            return result;
        }
    }
}
"@
}
function Resolve-ProspectiveDirectory([string]$Path) {
    $current = $Path
    $missing = @()
    while (-not (Test-Path -LiteralPath $current -PathType Container)) {
        if (Test-Path -LiteralPath $current) { throw 'An installation parent exists but is not a directory.' }
        $parent = [IO.Path]::GetDirectoryName($current)
        if (-not $parent -or $parent -eq $current) { throw 'Could not resolve an existing installation parent.' }
        $missing = @([IO.Path]::GetFileName($current)) + $missing
        $current = $parent
    }
    $resolved = [WaypointInstallerPath]::ResolveDirectory($current)
    foreach ($component in $missing) { $resolved = Join-Path $resolved $component }
    return [IO.Path]::GetFullPath($resolved)
}
try {
    $resolvedSource = [WaypointInstallerPath]::ResolveDirectory($source)
    $resolvedDestination = Resolve-ProspectiveDirectory $InstallDirectory
} catch { throw "Could not verify the installation path; nothing was copied. $($_.Exception.Message)" }
$resolvedPrefix = $resolvedSource.TrimEnd([char[]]'\/') + [IO.Path]::DirectorySeparatorChar
if ($resolvedDestination.Equals($resolvedSource, [StringComparison]::OrdinalIgnoreCase) -or
    $resolvedDestination.StartsWith($resolvedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Choose an installation directory outside the extracted package app folder.'
}
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
$officeFiles = @()
if ($null -ne $manifest.libreoffice_files) {
    $officeFiles = @($manifest.libreoffice_files.PSObject.Properties)
}
if ($officeFiles.Count -gt 0) {
    $officeLicense = @('dependencies/libreoffice/LICENSE', 'dependencies/libreoffice/LICENSE.txt', 'dependencies/libreoffice/LICENSE.html') | Where-Object { $manifest.libreoffice_files.$_ }
    if ($officeFiles.Count -gt 50000 -or -not $manifest.libreoffice_files.'dependencies/libreoffice/program/soffice.com' -or -not $officeLicense) {
        throw 'Package manifest contains an incomplete LibreOffice runtime inventory.'
    }
    foreach ($entry in $officeFiles) {
        $name = $entry.Name
        if (-not $name.StartsWith('dependencies/libreoffice/', [StringComparison]::Ordinal) -or $name -match '[\\:*?"<>|\x00-\x1f]' -or $name -match '(^|/)\.{1,2}(/|$)|//|[. ](/|$)' -or $entry.Value -notmatch '^[a-fA-F0-9]{64}$') {
            throw 'Package manifest contains an invalid LibreOffice runtime entry.'
        }
        $file = Join-Path $source $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Value) {
            throw "Bundled LibreOffice file is missing or damaged: $name. Extract the complete package and try again."
        }
    }
}
$shortcut = Join-Path $env:APPDATA 'Microsoft/Windows/Start Menu/Programs/Waypoint.lnk'
if (-not $NoShortcut -and (Test-Path -LiteralPath $shortcut)) { throw 'The Waypoint Start Menu shortcut already exists; it will not be overwritten.' }
New-Item -ItemType Directory -Path $InstallDirectory -ErrorAction Stop | Out-Null
try {
    # Enumerate with Force: wildcard expansion otherwise omits hidden package
    # entries, including dependencies that were verified above.
    foreach ($item in @(Get-ChildItem -LiteralPath $source -Force -ErrorAction Stop)) {
        Copy-Item -LiteralPath $item.FullName -Destination $InstallDirectory -Recurse -Force -ErrorAction Stop
    }
    # Confirm the actual installed files, not only the extracted source, before
    # creating a shortcut or reporting that installation completed.
    foreach ($entry in @(
        @{ Name = 'waypoint_web.exe'; Hash = $manifest.executable_sha256 },
        @{ Name = $manifest.openssl_dll; Hash = $manifest.openssl_sha256 }
    )) {
        $file = Join-Path $InstallDirectory $entry.Name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Hash) {
            throw "Installed package file is missing or damaged: $($entry.Name)."
        }
    }
    foreach ($entry in (@($pythonFiles) + @($officeFiles))) {
        $file = Join-Path $InstallDirectory $entry.Name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Value) {
            throw "Installed dependency file is missing or damaged: $($entry.Name)."
        }
    }
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
