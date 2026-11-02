param([Parameter(Mandatory=$true)][string]$InstallDirectory)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$root = (Resolve-Path (Join-Path $repo 'target')).Path
$installed = (Resolve-Path -LiteralPath $InstallDirectory).Path
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
  $tools = Invoke-RestMethod ($base + '/api/tool-status') -Method Get -Headers $headers
  $expectedPython = [IO.Path]::GetFullPath((Join-Path $installed 'dependencies/python/python.exe'))
  if (-not $tools.pdf_python -or [IO.Path]::GetFullPath($tools.pdf_python) -ne $expectedPython) { throw 'Bundled Python was not selected' }
  $started = Invoke-RestMethod ($base + '/api/pdf-probe/start') -Method Post -Headers $headers -ContentType 'application/json' -Body '{}'
  $deadline = [DateTime]::UtcNow.AddSeconds(30)
  do {
    Start-Sleep -Milliseconds 250
    $status = Invoke-RestMethod ($base + '/api/pdf-probe/status') -Method Post -Headers $headers -ContentType 'application/json' -Body (@{task_id=$started.task_id}|ConvertTo-Json -Compress)
  } while ($status.status -eq 'running' -and [DateTime]::UtcNow -lt $deadline)
  if ($status.status -ne 'complete') { throw ('Bundled PDF probe failed: ' + $status.error) }
  $headers['X-Document-Name'] = 'fictional.pdf'
  $fixture = Join-Path $repo 'apps/web_app/tests/fictional-partial-text.pdf'
  $document = Invoke-RestMethod ($base + '/api/documents/upload') -Method Post -Headers $headers -ContentType 'application/octet-stream' -Body ([IO.File]::ReadAllBytes($fixture))
  $headers.Remove('X-Document-Name')
  $result = Invoke-RestMethod ($base + '/api/documents/extract') -Method Post -Headers $headers -ContentType 'application/json' -Body (@{id=$document.id}|ConvertTo-Json -Compress)
  if (-not $result.text -or -not $result.text.Trim()) { throw 'The bundled worker did not extract fixture text' }
  Write-Output 'Installed app selected bundled Python, passed its PDF dependency check and extracted fictional PDF text.'
} finally {
  try {
    if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
    if ($scratch.StartsWith($root + [IO.Path]::DirectorySeparatorChar) -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
  } finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
  }
}
