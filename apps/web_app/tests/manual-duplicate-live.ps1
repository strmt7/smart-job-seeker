# Read three locally prepared records: one listing, a cross-site copy, and a similarly titled listing with a different requisition.
# The input stays outside the repository; this probe makes no external requests or application submissions.
param([Parameter(Mandatory=$true)][string]$InputPath)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$root = (Resolve-Path (Join-Path $repo 'target')).Path
$installed = (Resolve-Path (Join-Path $repo 'target/debug')).Path
$scratch = Join-Path $root ('packaged-pdf-probe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$process = $null
$variables = @('APPDATA','PATH','WAYPOINT_WORKSPACE_DIR','WAYPOINT_NO_BROWSER','WAYPOINT_NO_TRAY','WAYPOINT_PDF_PYTHON')
$previous = @{}
foreach ($name in $variables) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
  $env:APPDATA = Join-Path $scratch 'appdata'
  $env:WAYPOINT_WORKSPACE_DIR = Join-Path $scratch 'workspace'
  $env:WAYPOINT_NO_BROWSER = '1'
  $env:WAYPOINT_NO_TRAY = '1'
  $env:PATH = Join-Path $env:SystemRoot 'System32'
  Remove-Item Env:WAYPOINT_PDF_PYTHON -ErrorAction SilentlyContinue
  $log = Join-Path $scratch 'stdout.txt'
  $errlog = Join-Path $scratch 'stderr.txt'
  $process = Start-Process -FilePath (Join-Path $installed 'waypoint_web.exe') -WindowStyle Hidden -PassThru -RedirectStandardOutput $log -RedirectStandardError $errlog
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  $match = $null
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($process.HasExited) { throw 'Application exited during startup' }
    if (Test-Path -LiteralPath $log) { $contents = Get-Content -LiteralPath $log -Raw; if ($contents) { $match = [regex]::Match($contents, 'http://127\.0\.0\.1:(\d+)/#([a-f0-9]{64})') } }
    if ($match -and $match.Success) { break }
    Start-Sleep -Milliseconds 200
  }
  if (-not $match -or -not $match.Success) { throw 'Application startup timed out' }
  $base = 'http://127.0.0.1:' + $match.Groups[1].Value
  $headers = @{'X-Workspace-Token'=$match.Groups[2].Value}
  $records = @(Get-Content -LiteralPath $InputPath -Raw -Encoding UTF8 | ConvertFrom-Json)
  if ($records.Count -ne 3) { throw 'Expected three listing records' }
  $ids = @()
  foreach ($record in $records) {
    $added = Invoke-RestMethod ($base + '/api/jobs/add') -Method Post -Headers $headers -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json -Depth 8 -Compress)))
    if ($added.existing -or -not $added.id) { throw 'Listing import did not create a separate record' }
    $ids += $added.id
  }
  $deadline = [DateTime]::UtcNow.AddSeconds(20)
  do {
    Start-Sleep -Milliseconds 250
    $state = Invoke-RestMethod ($base + '/api/state') -Method Get -Headers $headers
  } while ($state.duplicate_index_pending -and [DateTime]::UtcNow -lt $deadline)
  $same = @($state.duplicates | Where-Object { @($_.job_a,$_.job_b) -contains $ids[0] -and @($_.job_a,$_.job_b) -contains $ids[1] })
  $other = @($state.duplicates | Where-Object { @($_.job_a,$_.job_b) -contains $ids[0] -and @($_.job_a,$_.job_b) -contains $ids[2] })
  if ($same.Count -ne 1 -or $same[0].status -ne 'pending') { throw 'Cross-site candidate was not pending review' }
  if ($other.Count -ne 1 -or $other[0].status -ne 'pending' -or $other[0].reason -notmatch 'Requisition identifiers differ') { throw 'Different-requisition candidate did not receive the review warning' }
  Write-Output ('Three listings imported; matching pair and distinct-ID pair remain pending review. Index pending: ' + $state.duplicate_index_pending)

} finally {
  try {
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    if ($scratch.StartsWith($root + [IO.Path]::DirectorySeparatorChar) -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
  } finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
  }
}
