param([int]$DurationSeconds=180)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8)
$output=Join-Path $workspace ('artifacts\benchmarks\game-live-'+$run)
New-Item -ItemType Directory -Path $output -Force|Out-Null
$journal=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\folder-runtime\current.json') -Raw|ConvertFrom-Json
if($journal.phase -ne 'active'){throw 'No active folder session'}
$status=Join-Path $journal.task 'status.json'
$processes=@(Get-CimInstance Win32_Process -Filter "Name='SparkingZERO.exe' OR Name='SparkingZERO-Win64-Shipping.exe'"|Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($journal.source+'\',[StringComparison]::OrdinalIgnoreCase)}|ForEach-Object {[pscustomobject]@{pid=$_.ProcessId;startedUtc=$_.CreationDate.ToUniversalTime().ToString('o');path=$_.ExecutablePath}})
$meta=[ordered]@{runId=$run;sessionId=$journal.id;source=$journal.source;startedUtc=[DateTime]::UtcNow.ToString('o');processes=$processes;scope='Folder cache API counters during live play; not physical disk I/O or comparative loading benchmark';sampleIntervalSeconds=1}
$meta|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $output 'metadata.json')
[IO.File]::WriteAllText((Join-Path $workspace 'artifacts\benchmarks\current-live-run.txt'),$output)
$clock=[Diagnostics.Stopwatch]::StartNew();$samples=0
while($clock.Elapsed.TotalSeconds -lt $DurationSeconds -and -not(Test-Path -LiteralPath (Join-Path $output 'stop'))){
 try{
  $s=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
  $cpu=@(foreach($p in $processes){$live=Get-Process -Id $p.pid -ErrorAction SilentlyContinue;if($live){[pscustomobject]@{pid=$p.pid;cpuSeconds=$live.CPU;workingSetBytes=$live.WorkingSet64}}})
  [ordered]@{utc=[DateTime]::UtcNow.ToString('o');elapsedSeconds=$clock.Elapsed.TotalSeconds;active=$s.active;paused=$s.paused;hits=$s.hits;misses=$s.misses;sourceReadBytes=$s.sourceReadBytes;ssdReadBytes=$s.ssdReadBytes;occupiedBytes=$s.occupiedBytes;fillBytes=$s.fillBytes;sequentialBypasses=$s.sequentialBypasses;durableWrites=$s.durableWrites;checksumFailures=$s.checksumFailures;cacheErrors=$s.cacheErrors;gameProcesses=$cpu}|ConvertTo-Json -Compress -Depth 5|Add-Content -LiteralPath (Join-Path $output 'samples.jsonl')
  $samples++
 }catch{[DateTime]::UtcNow.ToString('o')+' '+$_.Exception.Message|Add-Content -LiteralPath (Join-Path $output 'errors.log')}
 Start-Sleep -Seconds 1
}
[pscustomobject]@{output=$output;samples=$samples;elapsedSeconds=$clock.Elapsed.TotalSeconds}|ConvertTo-Json
