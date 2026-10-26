$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot cache-benchmark-report.ps1)
$root=Join-Path (Split-Path $PSScriptRoot -Parent) ('artifacts\cache-benchmark\report-tests\'+[Guid]::NewGuid().ToString('N'))
function Check($condition,$message){if(-not $condition){throw $message}}
$cases=@('slower','inconclusive','quick','wrong-byte','different-address','no-ssd','extra-callbacks','extra-callbacks-unsafe')
foreach($case in $cases){
 $output=Join-Path $root $case;[void][IO.Directory]::CreateDirectory($output)
 $n=if($case -eq 'quick'){2}else{6}
 function Entry($name,$seconds,$cached=$false){
  $csv="operation,offset,bytes,latency_ms,error,correct`n0,0,4096,1,0,1`n1,65536,4096,1,0,1`n"
  [IO.File]::WriteAllText((Join-Path $output ($name+'.csv')),$csv)
  [pscustomobject]@{phase=$name;measurement=[pscustomobject]@{passed=$true;seconds=$seconds;operations=2;bytes=8192;errors=0;p95Ms=1;p99Ms=1;MiBPerSecond=1};cacheDelta=[pscustomobject]@{requestedReadBytes=$(if($cached){8192}else{0});ssdReadBytes=$(if($cached){8192}else{0});sourceReadBytes=0;verifiedSsdLowerBoundBytes=$(if($cached){8192}else{0});cacheErrors=0;checksumFailures=0}}
 }
 $run=[pscustomobject]@{id=('0'*32);startedUtc='2000-01-01T00:00:00Z';error=$null;cancelled=$false;cleanupComplete=$true;parameters=@{pairs=$n;requests=2};hardware=@{testFixture=$true};build=@{};initialDirect=(Entry initial 1);firstPass=(Entry first 1);training=@((Entry training 1));references=@{ssdDirect=(Entry ssd .1);windowsWarmup=(Entry warmup .1);windowsWarm=(Entry windows .1)};pairs=@()}
 for($i=1;$i -le $n;$i++){$directSeconds=if($case -eq 'inconclusive'){if($i%2){.8}else{1.25}}else{.75};$run.pairs+=[pscustomobject]@{index=$i;direct=(Entry ('direct'+$i) $directSeconds);cached=(Entry ('cached'+$i) 1 $true)}}
 switch($case){
  'wrong-byte'{(Get-Content -Raw (Join-Path $output cached1.csv)).Replace('1,0,1','1,0,0')|Set-Content (Join-Path $output cached1.csv)}
  'different-address'{(Get-Content -Raw (Join-Path $output cached1.csv)).Replace('65536','131072')|Set-Content (Join-Path $output cached1.csv)}
  'no-ssd'{$run.pairs[0].cached.cacheDelta.verifiedSsdLowerBoundBytes=0}
  'extra-callbacks'{$d=$run.pairs[0].cached.cacheDelta;$d.requestedReadBytes=12288;$d.ssdReadBytes=12288;$d.verifiedSsdLowerBoundBytes=8192}
  'extra-callbacks-unsafe'{$d=$run.pairs[0].cached.cacheDelta;$d.requestedReadBytes=12288;$d.ssdReadBytes=8192;$d.sourceReadBytes=4096;$d.verifiedSsdLowerBoundBytes=4096}
 }
 $report=Write-BenchmarkReport $run $output
 if($case -in @('wrong-byte','different-address','no-ssd','extra-callbacks-unsafe')){Check (-not $report.valid -and -not $report.statistics -and -not $report.publicationReady) ('Invalid evidence accepted: '+$case)}
 else{Check $report.valid ('Valid fixture rejected: '+$case);Check ($report.publicationReady -eq ($n -ge 6)) 'Publication profile mismatch';Check ($report.statistics.verdict -eq $(if($case -eq 'inconclusive'){'inconclusive'}else{'slower'})) ('Verdict mismatch: '+$case)}
 Write-Output ('PASS '+$case)
}
[ordered]@{passed=$true;cases=$cases;scope='Synthetic report validation fixtures; not performance evidence'}|ConvertTo-Json|Set-Content (Join-Path $root result.json)
