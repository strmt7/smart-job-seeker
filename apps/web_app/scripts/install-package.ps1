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
if (-not (Test-Path -LiteralPath (Join-Path $source 'waypoint_web.exe'))) { throw 'Package is missing the application executable.' }
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
