param([ValidateRange(10,120)][int]$Seconds=60)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8)
$output=Join-Path $workspace ('artifacts\benchmarks\game-trace-'+$run)
[IO.Directory]::CreateDirectory($output)|Out-Null
$instance='SuperHDDGame-'+$run
$wpr=Join-Path $env:WINDIR 'System32\wpr.exe'
$xperf='C:\Program Files (x86)\Windows Kits\10\Windows Performance Toolkit\xperf.exe'
$owned=$false
$result=[ordered]@{runId=$run;instance=$instance;requestedSeconds=$Seconds;passed=$false;traceSaved=$false;error=$null;scope='Resource trace only; no claim of exact loading interval or comparative speedup';loadingEndpointsVerified=$false}
[IO.File]::WriteAllText((Join-Path $workspace 'artifacts\benchmarks\current-game-trace.txt'),$output)
function Save-State($phase){[pscustomobject]@{phase=$phase;output=$output;instance=$instance;utc=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $output 'state.json')}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();$principal=New-Object Security.Principal.WindowsPrincipal($identity)
 if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator token required for WPR CPU/DiskIO/FileIO tracing'}
 Save-State 'starting'
 # Use bounded memory-mode buffers and a unique instance, never stop/cancel
 # a recording belonging to another application or tracing session.
 & $wpr -start CPU.light -start DiskIO.light -start FileIO.light -instancename $instance 2>&1|Out-File -LiteralPath (Join-Path $output 'start.log')
 if($LASTEXITCODE -ne 0){throw ('WPR start failed: '+$LASTEXITCODE)}
 $owned=$true
 $j=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\folder-runtime\current.json') -Raw|ConvertFrom-Json
 $status=Join-Path $j.task 'status.json'
 $clock=[Diagnostics.Stopwatch]::StartNew()
 [ordered]@{utc=[DateTime]::UtcNow.ToString('o');qpc=[Diagnostics.Stopwatch]::GetTimestamp();qpcFrequency=[Diagnostics.Stopwatch]::Frequency;session=$j;profiles=@('CPU.light','DiskIO.light','FileIO.light');bufferMode='bounded memory';computerUseCapture='Failed: window capture timed out';cacheStateChanged=$false;gameStartedOrStoppedByRecorder=$false}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $output 'metadata.json')
 Save-State 'recording'
 while($clock.Elapsed.TotalSeconds -lt $Seconds -and -not(Test-Path -LiteralPath (Join-Path $output 'stop'))){
  $s=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
  $p=@(Get-Process -Name 'SparkingZERO-Win64-Shipping','folder-worker' -ErrorAction SilentlyContinue|ForEach-Object {[pscustomobject]@{pid=$_.Id;name=$_.ProcessName;cpuSeconds=$_.CPU;workingSetBytes=$_.WorkingSet64}})
  [ordered]@{utc=[DateTime]::UtcNow.ToString('o');elapsedSeconds=$clock.Elapsed.TotalSeconds;active=$s.active;paused=$s.paused;hits=$s.hits;misses=$s.misses;sourceReadBytes=$s.sourceReadBytes;ssdReadBytes=$s.ssdReadBytes;occupiedBytes=$s.occupiedBytes;cacheErrors=$s.cacheErrors;checksumFailures=$s.checksumFailures;processes=$p}|ConvertTo-Json -Compress -Depth 5|Add-Content -LiteralPath (Join-Path $output 'samples.jsonl')
  Start-Sleep -Seconds 1
 }
 $result.recordedSeconds=$clock.Elapsed.TotalSeconds
}catch{$result.error=$_.Exception.Message}finally{
 if($owned){
  Save-State 'saving'
  & $wpr -status collectors -details -instancename $instance 2>&1|Out-File -LiteralPath (Join-Path $output 'collector-status.log')
  & $wpr -stop (Join-Path $output 'trace.etl') -skipPdbGen -instancename $instance 2>&1|Out-File -LiteralPath (Join-Path $output 'stop.log')
  $result.traceSaved=($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath (Join-Path $output 'trace.etl')))
  if($result.traceSaved){
   Save-State 'exporting'
   foreach($action in @('tracestats','diskio','profile','process','hardfault')){
    & $xperf -i (Join-Path $output 'trace.etl') -o (Join-Path $output ($action+'.csv')) -a $action 2>&1|Out-File -LiteralPath (Join-Path $output ($action+'-export.log'))
    if($LASTEXITCODE -ne 0){$result['exportError_'+$action]=$LASTEXITCODE}
   }
  }elseif(-not $result.error){$result.error='WPR could not save the owned trace; consult stop.log'}
 }
 $result.passed=($result.traceSaved -and -not $result.error)
 $result|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $output 'result.json')
 Save-State $(if($result.passed){'completed'}else{'failed'})
}
if(-not $result.passed){exit 1}
