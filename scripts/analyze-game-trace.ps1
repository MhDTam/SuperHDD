$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=(Get-Content -LiteralPath (Join-Path $workspace 'artifacts\benchmarks\current-game-trace.txt') -Raw).Trim()
if(-not $run.StartsWith((Join-Path $workspace 'artifacts\benchmarks\game-trace-'),[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected trace location'}
function Read-TrimmedCsv($path){$lines=@(Get-Content -LiteralPath $path|Where-Object {$_ -match '\S'});foreach($row in ($lines|ConvertFrom-Csv)){$entry=[ordered]@{};foreach($p in $row.PSObject.Properties){$entry[$p.Name.Trim()]=([string]$p.Value).Trim()};[pscustomobject]$entry}}
function Sum($rows,$property){$n=0.0;foreach($r in $rows){$n+=[double]$r.$property};$n}
$samples=@(Get-Content -LiteralPath (Join-Path $run 'samples.jsonl')|ForEach-Object {$_|ConvertFrom-Json})
if($samples.Count -lt 2){throw 'Insufficient samples'}
$first=$samples[0];$last=$samples[-1];$seconds=[double]$last.elapsedSeconds-[double]$first.elapsedSeconds
$stats=Get-Content -LiteralPath (Join-Path $run 'tracestats.csv') -Raw
$lost=[regex]::Match($stats,'Total # Lost Events\s*:\s*(\d+)')
if(-not $lost.Success){throw 'Missing event loss information'}
$disk=@(Read-TrimmedCsv (Join-Path $run 'disk-summary.csv'))
$cpu=@(Read-TrimmedCsv (Join-Path $run 'cpu-detail.csv'))
$worker=@($first.processes|Where-Object name -eq 'folder-worker')[0]
$workerLast=@($last.processes|Where-Object pid -eq $worker.pid)[0]
if(-not $worker -or -not $workerLast){throw 'Worker process changed during interval'}
$cores=[int]([regex]::Match($stats,'Number of Processors\s*:\s*(\d+)').Groups[1].Value)
$cpuSeconds=[double]$workerLast.cpuSeconds-[double]$worker.cpuSeconds
$cacheRows=@($disk|Where-Object {$_.Filename -match '\\folder-runtime\\[a-f0-9]{32}\\cache\.bin$'})
$pageRows=@($disk|Where-Object {$_.Filename -eq 'D:\pagefile.sys'})
$otherDRows=@($disk|Where-Object {$_.Filename -like 'D:\*' -and $_.Filename -ne 'D:\pagefile.sys'})
$workerCpu=@($cpu|Where-Object {$_.'Process Name ( PID)' -eq ('folder-worker.exe ('+$worker.pid+')')})
$report=[ordered]@{
 scope='Single resource calibration trace; no verified loading endpoints or comparative speedup'
 sampleSeconds=$seconds
 traceLostEvents=[int]$lost.Groups[1].Value
 sourceReadBytesDelta=([long]$last.sourceReadBytes-[long]$first.sourceReadBytes)
 ssdReadBytesDelta=([long]$last.ssdReadBytes-[long]$first.ssdReadBytes)
 hitsDelta=([long]$last.hits-[long]$first.hits)
 missesDelta=([long]$last.misses-[long]$first.misses)
 cacheErrorsDelta=([long]$last.cacheErrors-[long]$first.cacheErrors)
 checksumFailuresDelta=([long]$last.checksumFailures-[long]$first.checksumFailures)
 workerCpuSeconds=$cpuSeconds
 workerCpuPercentOfAllLogicalProcessors=($cpuSeconds/$seconds/$cores*100)
 workerCpuPercentOfOneLogicalProcessor=($cpuSeconds/$seconds*100)
 workerWorkingSetMiB=([long]$workerLast.workingSetBytes/1MB)
 etwFullTrace=@{
  intervalNote='Full ETW interval includes stop/rundown; distinct from sampler interval'
  cachePhysicalReadBytes=(Sum $cacheRows 'R-Size')
  cachePhysicalReadOperations=(Sum $cacheRows 'R-Count')
  dPagefilePhysicalReadBytes=(Sum $pageRows 'R-Size')
  dOtherNamedFilePhysicalReadBytes=(Sum $otherDRows 'R-Size')
  dOtherNamedFileRows=$otherDRows.Count
  workerSampledCpuPercentRounded=(Sum $workerCpu 'Usage %')
 }
 loadingEndpointsVerified=$false
 loadingSpeedupEstablished=$false
 visualMeasurementBlocked='Window capture timed out, including after windowed-mode change and activation'
}
$report|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $run 'analysis.json')
$report|ConvertTo-Json -Depth 7
