param(
  [Parameter(Mandatory=$true)][string]$PythonPath,
  [string]$OfficeRoot = (Join-Path $env:ProgramFiles 'LibreOffice')
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$root = (Resolve-Path (Join-Path $repo 'target')).Path
$scratch = Join-Path $root ('office-probe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$process = $null
$variables = @('APPDATA','WAYPOINT_WORKSPACE_DIR','WAYPOINT_NO_BROWSER','WAYPOINT_NO_TRAY','WAYPOINT_PDF_PYTHON','WAYPOINT_SOFFICE')
$previous = @{}
foreach ($name in $variables) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
  $office = $OfficeRoot
  if (-not (Test-Path -LiteralPath (Join-Path $office 'program\soffice.com'))) { throw 'LibreOffice unavailable' }
  $junction = Join-Path $scratch 'custom-install'
  New-Item -ItemType Junction -Path $junction -Target $office | Out-Null
  $env:APPDATA = Join-Path $scratch 'appdata'
  $env:WAYPOINT_WORKSPACE_DIR = Join-Path $scratch 'workspace'
  $env:WAYPOINT_NO_BROWSER = '1'
  $env:WAYPOINT_NO_TRAY = '1'
  $env:WAYPOINT_PDF_PYTHON = $PythonPath
  Remove-Item Env:WAYPOINT_SOFFICE -ErrorAction SilentlyContinue
  $log = Join-Path $scratch 'stdout.txt'
  $errlog = Join-Path $scratch 'stderr.txt'
  $process = Start-Process -FilePath (Join-Path $root 'debug/waypoint_web.exe') -WindowStyle Hidden -PassThru -RedirectStandardOutput $log -RedirectStandardError $errlog
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
  $chosen = Join-Path $junction 'program\soffice.com'
  $setting = Invoke-RestMethod ($base + '/api/office-path') -Method Post -Headers $headers -ContentType 'application/json' -Body (@{path=$chosen}|ConvertTo-Json -Compress)
  if ($setting.active -ne $chosen) { throw 'Custom candidate was not selected' }
  $observed = Invoke-RestMethod ($base + '/api/tool-status') -Method Get -Headers $headers
  if ($observed.office_setting.path -ne $chosen -or $observed.office_setting.available -ne $true) { throw 'Saved custom path was not reported' }
  $headers['X-Document-Name'] = 'fictional.txt'
  $document = Invoke-RestMethod ($base + '/api/documents/upload') -Method Post -Headers $headers -ContentType 'application/octet-stream' -Body ([Text.Encoding]::UTF8.GetBytes('Fictional CV: Avery Example.'))
  $headers.Remove('X-Document-Name')
  $started = Invoke-RestMethod ($base + '/api/documents/convert-pdf-start') -Method Post -Headers $headers -ContentType 'application/json' -Body (@{id=$document.id}|ConvertTo-Json -Compress)
  $deadline = [DateTime]::UtcNow.AddSeconds(45)
  do {
    Start-Sleep -Milliseconds 250
    $status = Invoke-RestMethod ($base + '/api/documents/convert-pdf-status') -Method Post -Headers $headers -ContentType 'application/json' -Body (@{task_id=$started.task_id}|ConvertTo-Json -Compress)
  } while ($status.status -eq 'running' -and [DateTime]::UtcNow -lt $deadline)
  if ($status.status -ne 'complete') { throw ('Conversion did not complete: ' + $status.status + '; ' + $status.error) }
  if ($status.text_check.status -ne 'matches') { throw 'The converted PDF did not pass the text comparison' }
  Remove-Item -LiteralPath $junction -Force
  $observed = Invoke-RestMethod ($base + '/api/tool-status') -Method Get -Headers $headers
  if ($observed.office_setting.path -ne $chosen -or $observed.office_setting.available -ne $false) { throw 'Missing custom path was not reported' }
  Write-Output 'Isolated custom-path conversion and text comparison passed.'
} finally {
  try {
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    if ($junction -and (Test-Path -LiteralPath $junction)) { Remove-Item -LiteralPath $junction -Force }
    if ($scratch.StartsWith($root + [IO.Path]::DirectorySeparatorChar) -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
  } finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
  }
}
