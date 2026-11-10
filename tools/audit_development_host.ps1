param(
    [string]$Output,
    [string[]]$ExtraIdentifier = @(),
    [switch]$CheckOllama
)
# Read-only observations. Matching processes are candidates, not proof of staleness.
$ErrorActionPreference = 'Stop'
$auditRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$auditTarget = Join-Path $auditRoot 'target'
if (-not (Test-Path -LiteralPath $auditTarget -PathType Container) -or
    ((Get-Item -LiteralPath $auditTarget -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'A regular repository target directory is required.'
}
if (-not $Output) { $Output = Join-Path $auditTarget ('development-host-' + [guid]::NewGuid().ToString('N') + '.private.json') }
$auditOutput = [IO.Path]::GetFullPath($Output)
if (-not [string]::Equals([IO.Path]::GetDirectoryName($auditOutput), $auditTarget, [StringComparison]::OrdinalIgnoreCase) -or
    -not $auditOutput.EndsWith('.private.json', [StringComparison]::OrdinalIgnoreCase) -or (Test-Path -LiteralPath $auditOutput)) {
    throw 'Choose a new .private.json file directly inside this repository target directory.'
}
$auditIdentifiers = @($auditRoot, $auditRoot.Replace('\','/')) + $ExtraIdentifier
if (@($auditIdentifiers | Where-Object { $_.Length -lt 4 -or $_.Length -gt 1024 }).Count) { throw 'Invalid audit identifier.' }
$auditPattern = ($auditIdentifiers | ForEach-Object { [regex]::Escape($_) }) -join '|'
$auditErrors = [System.Collections.Generic.List[string]]::new()
$audit = @{time=[datetime]::UtcNow.ToString('o'); processes=@();services=@();tasks=@();startup=@();resident_models=$null;model_state='not_assessed';available_ram_bytes=$null}
try {
    $audit.processes = @(Get-CimInstance Win32_Process -OperationTimeoutSec 5 | Where-Object {
        $_.ProcessId -ne $PID -and ($_.CommandLine -match $auditPattern -or $_.ExecutablePath -match $auditPattern)
    } | Select-Object ProcessId,ParentProcessId,CreationDate,Name,ExecutablePath)
} catch { $auditErrors.Add('Process inventory unavailable') }
try {
    $audit.services = @(Get-CimInstance Win32_Service -OperationTimeoutSec 5 | Where-Object { $_.PathName -match $auditPattern } | Select-Object Name,State,StartMode)
} catch { $auditErrors.Add('Service inventory unavailable') }
try {
    $audit.tasks = @(Get-ScheduledTask | Where-Object { ($_.Actions | Out-String) -match $auditPattern } | Select-Object TaskName,TaskPath,State)
} catch { $auditErrors.Add('Scheduled task inventory unavailable') }
foreach ($auditHive in @('HKCU','HKLM')) {
    foreach ($auditKey in @('Software\Microsoft\Windows\CurrentVersion\Run','Software\Microsoft\Windows\CurrentVersion\RunOnce','Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run','Software\WOW6432Node\Microsoft\Windows\CurrentVersion\RunOnce')) {
        try {
            $auditRegistry = $auditHive + ':\' + $auditKey
            if (Test-Path -LiteralPath $auditRegistry) {
                $auditValues = Get-ItemProperty -LiteralPath $auditRegistry
                foreach ($auditProperty in $auditValues.PSObject.Properties) {
                    if ($auditProperty.Name -notlike 'PS*' -and [string]$auditProperty.Value -match $auditPattern) {
                        $audit.startup += @{hive=$auditHive;key=$auditKey;name=$auditProperty.Name}
                    }
                }
            }
        } catch { $auditErrors.Add('Startup inventory unavailable: ' + $auditHive) }
    }
}
try {
    $audit.startup += @(Get-CimInstance Win32_StartupCommand -OperationTimeoutSec 5 | Where-Object { $_.Command -match $auditPattern } | Select-Object Name,Location)
} catch { $auditErrors.Add('Startup command inventory unavailable') }
if ($CheckOllama) {
    try {
        $auditModels = Invoke-RestMethod -Uri 'http://127.0.0.1:11434/api/ps' -TimeoutSec 5
        if (-not ($auditModels.models -is [array])) { throw 'Invalid resident-model response' }
        $audit.resident_models = $auditModels.models.Count; $audit.model_state = 'observed'
    } catch { $audit.model_state = 'unavailable'; $auditErrors.Add('Resident model inventory unavailable') }
}
try { $audit.available_ram_bytes = [uint64](Get-CimInstance Win32_OperatingSystem -OperationTimeoutSec 5).FreePhysicalMemory * 1024 }
catch { $auditErrors.Add('Available RAM observation unavailable') }
$audit.errors = @($auditErrors.ToArray())
$auditBytes = [Text.UTF8Encoding]::new($false).GetBytes(($audit | ConvertTo-Json -Depth 6))
$auditFile = [IO.File]::Open($auditOutput, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try { $auditFile.Write($auditBytes,0,$auditBytes.Length) } finally { $auditFile.Dispose() }
@{snapshot=$auditOutput;processes=$audit.processes.Count;services=$audit.services.Count;tasks=$audit.tasks.Count;startup=$audit.startup.Count;resident_models=$audit.resident_models;model_state=$audit.model_state;available_ram_bytes=$audit.available_ram_bytes;errors=$audit.errors} | ConvertTo-Json -Depth 3
if ($auditErrors.Count) { exit 1 }
