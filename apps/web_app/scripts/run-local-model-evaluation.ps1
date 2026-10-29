param([string]$Model = 'qwen3.5:9b')
$ErrorActionPreference='Stop'
if ($env:OS -ne 'Windows_NT') { throw 'This evaluation runner requires Windows resource counters.' }
if ([string]::IsNullOrWhiteSpace($Model)) { throw 'A local model name is required.' }
$repo=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
$fixtureDir=Join-Path $repo 'apps/web_app/tests'
$cases=@(Get-Content -LiteralPath (Join-Path $fixtureDir 'local-model-eval-cases.json') -Raw | ConvertFrom-Json)
if($cases.Count -ne 11){throw 'Evaluation fixture count does not match the grader.'}
$tags=Invoke-RestMethod 'http://127.0.0.1:11434/api/tags' -TimeoutSec 5
$matching=@($tags.models|Where-Object {$_.name -ceq $Model -and -not $_.remote_model -and -not $_.remote_host})
if($matching.Count -ne 1){throw 'The selected model must be installed locally; cloud models are not evaluated.'}
function Resources {
 $os=Get-CimInstance Win32_OperatingSystem
 $gpu=(Get-Counter -Counter '\GPU Adapter Memory(*)\Dedicated Usage' -ErrorAction Stop).CounterSamples|Measure-Object -Property CookedValue -Maximum
 if($null -eq $gpu.Maximum){throw 'Dedicated GPU memory telemetry unavailable.'}
 return @{free_system_bytes=[long]$os.FreePhysicalMemory*1024;gpu_dedicated_bytes=[long]$gpu.Maximum}
}
function Resident {
 $status=Invoke-RestMethod 'http://127.0.0.1:11434/api/ps' -TimeoutSec 5
 $all=@($status.models)
 if($all.Count -ne 1 -or $all[0].name -cne $Model -or -not $all[0].size -or $all[0].size_vram -ne $all[0].size -or $all[0].size -gt 14GB -or $all[0].context_length -ne 4096){throw 'Exactly the selected model must already be fully GPU-resident within 14 GiB with a 4096-token context.'}
 return $all[0]
}
$initial=Resources
if($initial.free_system_bytes -lt 12GB -or $initial.gpu_dedicated_bytes -gt 12GB){throw 'Evaluation resource thresholds are not satisfied.'}
$null=Resident
$outputDir=Join-Path $repo 'target/model-eval';New-Item -ItemType Directory -Force $outputDir|Out-Null
$path=Join-Path $outputDir ('evaluation-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'-'+[guid]::NewGuid().ToString('N')+'.jsonl')
function Save($value){$value|ConvertTo-Json -Depth 20 -Compress|Add-Content -LiteralPath $path -Encoding UTF8}
Save @{type='start';time=(Get-Date).ToUniversalTime().ToString('o');model=$Model;model_digest=$matching[0].digest;initial_resources=$initial;num_ctx=4096;keep_alive='30s';note='Fictional fixtures, local Ollama, sequential; no sites or application submissions.'}
foreach($case in $cases){
 try {
  $resources=Resources
  if($resources.free_system_bytes -lt 12GB -or $resources.gpu_dedicated_bytes -gt 12GB){throw 'Resource threshold reached.'}
  $null=Resident
  $request=@{model=$Model;prompt=$case.prompt;stream=$false;format='json';think=$false;keep_alive='30s';options=@{num_ctx=4096;num_predict=$case.num_predict;temperature=0}}
  if($case.image){if($case.image -notin @('fictional-form.png','fictional-dense-form.png')){throw 'Invalid fixture image.'};$request.images=@([Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $fixtureDir $case.image))))}
  $timer=[Diagnostics.Stopwatch]::StartNew()
  $response=Invoke-RestMethod 'http://127.0.0.1:11434/api/generate' -Method Post -ContentType 'application/json' -Body ([Text.Encoding]::UTF8.GetBytes(($request|ConvertTo-Json -Depth 8 -Compress))) -TimeoutSec 120
  $timer.Stop();$resident=Resident
  Save @{type='case';name=$case.name;prompt=$case.prompt;raw_response=$response.response;done=$response.done;done_reason=$response.done_reason;elapsed_ms=$timer.ElapsedMilliseconds;resources=(Resources);resident=@($resident);image=$case.image}
 } catch {Save @{type='error';name=$case.name;error=$_.Exception.Message;time=(Get-Date).ToUniversalTime().ToString('o')};throw "Evaluation stopped at $($case.name); partial receipt: $path. $($_.Exception.Message)"}
}
Write-Output $path
