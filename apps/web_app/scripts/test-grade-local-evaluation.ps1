$ErrorActionPreference='Stop'
$root=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$testDir=Join-Path $root 'target/model-eval';New-Item -ItemType Directory -Force $testDir|Out-Null
$fixture=Join-Path $testDir 'grader-negative-fixture.jsonl'
$items=@(
 @{type='case';name='field_mapping';done=$true;done_reason='stop';raw_response='{"mappings":[{"selector":"#given","profile_key":"last_name"}]}'},
 @{type='case';name='form_visual';done=$true;done_reason='stop';raw_response='{"observations":[{"label":"Full name","visible_value":"Incorrect"}]}'},
 @{type='case';name='same_vacancy';done=$true;done_reason='stop';raw_response='{"verdict":"possibly_same"}'},
 @{type='case';name='uncertain_answer';done=$true;done_reason='length';raw_response='{"text":"","facts":[]}'}
)
$items|ForEach-Object { $_|ConvertTo-Json -Compress -Depth 8 }|Set-Content -LiteralPath $fixture -Encoding utf8
$result=& (Join-Path $PSScriptRoot 'grade-local-evaluation.ps1') -Receipt @($fixture)|ConvertFrom-Json
foreach($name in @('field_mapping','form_visual','same_vacancy','uncertain_answer')){if(($result|Where-Object case -CEQ $name).status -cne 'fail'){throw "Grader accepted invalid fixture: $name"}}
if(($result|Where-Object case -CEQ 'job_extraction').status -cne 'missing'){throw 'Grader did not report a missing case.'}
Write-Host 'Local-model grading negative checks passed.'
