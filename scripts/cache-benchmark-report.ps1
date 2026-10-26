function Write-BenchmarkReport($run,$output){
 $pairs=@($run.pairs);$valid=$run.error -eq $null -and -not $run.cancelled -and $run.cleanupComplete -and $pairs.Count -eq $run.parameters.pairs
 foreach($pair in $pairs){
  $d=$pair.cached.cacheDelta;$bound=[Math]::Max(0,[long]$d.ssdReadBytes-[Math]::Max(0,[long]$d.requestedReadBytes-[long]$pair.cached.measurement.bytes))
  if($d.verifiedSsdLowerBoundBytes -ne $bound -or $d.sourceReadBytes -lt 0 -or $d.ssdReadBytes -lt 0){$valid=$false}
  if(-not $pair.direct.measurement.passed -or -not $pair.cached.measurement.passed -or $pair.cached.cacheDelta.verifiedSsdLowerBoundBytes -lt $pair.cached.measurement.bytes*.95 -or $pair.cached.cacheDelta.requestedReadBytes -lt $pair.cached.measurement.bytes -or ($pair.cached.cacheDelta.ssdReadBytes+$pair.cached.cacheDelta.sourceReadBytes) -ne $pair.cached.cacheDelta.requestedReadBytes -or $pair.direct.cacheDelta.requestedReadBytes -ne 0 -or $pair.cached.cacheDelta.cacheErrors -or $pair.cached.cacheDelta.checksumFailures){$valid=$false}
  $addresses=@{}
  foreach($arm in @('direct','cached')){
   $entry=$pair.$arm;$file=Join-Path $output ($entry.phase+'.csv')
   if(-not (Test-Path -LiteralPath $file)){$valid=$false;continue}
   $samples=@(Import-Csv -LiteralPath $file)
   if($samples.Count -ne $entry.measurement.operations -or $entry.measurement.operations -ne $run.parameters.requests -or @($samples|Where-Object {$_.correct -ne '1' -or $_.error -ne '0'}).Count -or ($samples|Measure-Object -Property bytes -Sum).Sum -ne $entry.measurement.bytes){$valid=$false}
   $addresses[$arm]=($samples|ForEach-Object {$_.operation+':'+$_.offset+':'+$_.bytes}) -join '|'
  }
  if($addresses.direct -ne $addresses.cached){$valid=$false}
 }
 $statistics=$null
 $evidence=@($run.initialDirect,$run.firstPass)+@($run.training)+@($run.references.ssdDirect,$run.references.windowsWarmup,$run.references.windowsWarm)
 foreach($entry in $evidence){
  if(-not $entry -or -not $entry.measurement.passed -or $entry.measurement.errors -or $entry.measurement.seconds -le 0){$valid=$false;continue}
  $file=Join-Path $output ($entry.phase+'.csv')
  if(-not (Test-Path -LiteralPath $file)){$valid=$false;continue}
  $samples=@(Import-Csv -LiteralPath $file)
  if($samples.Count -ne $entry.measurement.operations -or @($samples|Where-Object {$_.correct -ne '1' -or $_.error -ne '0'}).Count -or ($samples|Measure-Object -Property bytes -Sum).Sum -ne $entry.measurement.bytes){$valid=$false}
 }
 foreach($pair in $pairs){foreach($arm in @('direct','cached')){if($pair.$arm.measurement.seconds -le 0 -or [double]::IsNaN($pair.$arm.measurement.seconds) -or [double]::IsInfinity($pair.$arm.measurement.seconds)){$valid=$false}}}
 if($valid -and $pairs.Count -ge 2){
  $logs=@($pairs|ForEach-Object {[Math]::Log($_.direct.measurement.seconds/$_.cached.measurement.seconds)})
  $mean=($logs|Measure-Object -Average).Average;$sum=0.0;foreach($x in $logs){$sum+=($x-$mean)*($x-$mean)}
  $critical=if($pairs.Count -ge 10){2.262157}else{if($pairs.Count -ge 6){2.570582}else{12.706205}}
  $margin=$critical*[Math]::Sqrt($sum/($pairs.Count-1)/$pairs.Count)
  $lower=[Math]::Exp($mean-$margin);$upper=[Math]::Exp($mean+$margin);$ratio=[Math]::Exp($mean)
  $verdict=if($lower -gt 1){'faster'}elseif($upper -lt 1){'slower'}else{'inconclusive'}
  $statistics=[ordered]@{pairedGeometricMeanSpeedup=$ratio;timeReductionPercent=(1-1/$ratio)*100;confidence95Lower=$lower;confidence95Upper=$upper;confidenceMethod='Student t interval on paired log wall-time ratios; rounds are not independent hardware samples';verdict=$verdict}
 }
 $public=[ordered]@{schemaVersion=1;benchmark='Super HDD storage benchmark';runId=$run.id;startedUtc=$run.startedUtc;finishedUtc=[DateTime]::UtcNow.ToString('o');valid=$valid;publicationReady=($valid -and $pairs.Count -ge 6);parameters=$run.parameters;hardware=$run.hardware;build=$run.build;initialDirect=$run.initialDirect;firstPass=$run.firstPass;training=$run.training;pairs=$pairs;statistics=$statistics;cleanupComplete=$run.cleanupComplete;cancelled=$run.cancelled;error=$run.error;limitations=@('Synthetic aligned archive I/O, not game load time, FPS, or anti-cheat certification.','Uses a separately compiled storage-isolated build of the same worker/pool source; ordinary game cache workers are not replaced.','Windows data caching is bypassed for the direct client, worker source and SSD cache reader. Index metadata remains memory mapped. Disk/controller caches are not purged.','Warm-cache results follow explicit training; first-pass and training costs are retained separately.','Wall time includes identical client validation work. Read latency percentiles exclude that client verification.','An interval containing 1x is inconclusive. A slowdown or failed run is retained, not hidden.','This machine and repeated synthetic workload cannot establish performance in all games or on other hardware.')}
 $public['references']=$run.references
 $share=Join-Path $output share;[void][IO.Directory]::CreateDirectory($share)
 $json=$public|ConvertTo-Json -Depth 18
 [IO.File]::WriteAllText((Join-Path $share report.json),$json,[Text.UTF8Encoding]::new($false))
 function Escape-BenchmarkHtml($s){[Net.WebUtility]::HtmlEncode([string]$s)}
 $rows=foreach($pair in $pairs){foreach($arm in @('direct','cached')){$m=$pair.$arm.measurement;'<tr><td>'+($pair.index)+'</td><td>'+($arm)+'</td><td>'+([double]$m.seconds).ToString('F4',[Globalization.CultureInfo]::InvariantCulture)+'</td><td>'+([double]$m.MiBPerSecond).ToString('F2',[Globalization.CultureInfo]::InvariantCulture)+'</td><td>'+([double]$m.p95Ms).ToString('F3',[Globalization.CultureInfo]::InvariantCulture)+'</td><td>'+([double]$m.p99Ms).ToString('F3',[Globalization.CultureInfo]::InvariantCulture)+'</td><td>'+(Escape-BenchmarkHtml $m.passed)+'</td></tr>'}}
 $headline=if(-not $valid){'INVALID / INCOMPLETE — no performance claim'}elseif(-not $public.publicationReady){'Exploratory run — use Standard for publication'}else{'Valid paired measurement: '+$statistics.verdict}
 $number=if($statistics){'<p class="ratio">'+([double]$statistics.pairedGeometricMeanSpeedup).ToString('F2',[Globalization.CultureInfo]::InvariantCulture)+'× <small>paired geometric mean speed ratio</small></p><p>Approximate 95% interval: '+([double]$statistics.confidence95Lower).ToString('F2',[Globalization.CultureInfo]::InvariantCulture)+'–'+([double]$statistics.confidence95Upper).ToString('F2',[Globalization.CultureInfo]::InvariantCulture)+'×. Time reduction: '+([double]$statistics.timeReductionPercent).ToString('F1',[Globalization.CultureInfo]::InvariantCulture)+'% (negative means slower).</p>'}else{''}
 $limits=($public.limitations|ForEach-Object {'<li>'+(Escape-BenchmarkHtml $_)+'</li>'}) -join ''
 $html='<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Super HDD benchmark</title><style>body{font:16px/1.6 system-ui;margin:40px auto;max-width:1080px;padding:0 24px;color:#18243b;background:#f5f7fb}section{background:white;border:1px solid #dae1eb;border-radius:16px;padding:24px;margin:20px 0}h1{font-size:32px}h2{font-size:20px}.ratio{font-size:48px;font-weight:bold}.ratio small{font-size:16px;display:block;font-weight:normal}table{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}td,th{text-align:left;padding:10px;border-bottom:1px solid #dde4ee}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px}a{color:#285cab}@media print{body{background:white;margin:0}section{break-inside:avoid}}</style><h1>Super HDD storage benchmark</h1><p>Run '+(Escape-BenchmarkHtml $run.id)+' · '+(Escape-BenchmarkHtml $run.startedUtc)+'</p><section><h2>'+(Escape-BenchmarkHtml $headline)+'</h2>'+$number+'<p>'+(Escape-BenchmarkHtml $run.error)+'</p></section><section><h2>Every paired round</h2><table><tr><th>Pair</th><th>Arm</th><th>Wall seconds</th><th>MiB/s</th><th>p95 ms</th><th>p99 ms</th><th>Data correct</th></tr>'+($rows -join '')+'</table></section><section><h2>Method and limits</h2><ul>'+$limits+'</ul><p>Same generated archive, seed, aligned addresses, sizes and client in each arm; AB/BA order alternates by pair. Training is outside the warm measurement. All requested bytes are checked against a deterministic position-dependent pattern.</p><p><a href="https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering">Microsoft: file buffering and hardware-cache limitations</a></p></section><section><h2>Reproduction and evidence</h2><p>See report.json, samples.csv, source/, and manifest.json in this package. Do not convert these ratios into game-loading or FPS claims.</p><pre>'+(Escape-BenchmarkHtml (($public.parameters,$public.hardware,$public.build)|ConvertTo-Json -Depth 9))+'</pre></section></html>'
 $ref='<section><h2>Reference checks: hardware and Windows caching</h2><p>These are single reference readings, not the paired speed estimate.</p><p>Direct SSD reference: '+([double]$run.references.ssdDirect.measurement.seconds).ToString('F5',[Globalization.CultureInfo]::InvariantCulture)+' s. Windows-buffered pre-read HDD file: '+([double]$run.references.windowsWarm.measurement.seconds).ToString('F5',[Globalization.CultureInfo]::InvariantCulture)+' s.</p><p>Compare these with the engine times. A fast warm Windows cache can remove the apparent storage benefit; direct SSD shows the engine overhead above the device. These references do not measure game loading.</p></section>'
 if($run.references.ssdDirect -and $run.references.windowsWarm){$html=$html.Replace('<section><h2>Every paired round</h2>',$ref+'<section><h2>Every paired round</h2>')}
 $costRows=foreach($entry in @($run.initialDirect,$run.firstPass)+@($run.training)){if($entry){'<tr><td>'+(Escape-BenchmarkHtml $entry.phase)+'</td><td>'+([double]$entry.measurement.seconds).ToString('F5',[Globalization.CultureInfo]::InvariantCulture)+'</td><td>'+(Escape-BenchmarkHtml $entry.measurement.bytes)+'</td></tr>'}}
 $cost='<section><h2>First pass and training cost</h2><p>Read-call wall times below are separate from the warm result. Fixture creation and deliberate training waits are excluded; started/finished timestamps retain the full run duration. Training uses 64 KiB requests and a half-order rotation to exercise the unchanged second-touch admission policy.</p><table><tr><th>Phase</th><th>Wall seconds</th><th>Bytes read</th></tr>'+($costRows -join '')+'</table></section>'
 $html=$html.Replace('<section><h2>Method and limits</h2>',$cost+'<section><h2>Method and limits</h2>')
 [IO.File]::WriteAllText((Join-Path $share report.html),$html,[Text.UTF8Encoding]::new($false))
 $csvs=@(Get-ChildItem -LiteralPath $output -Filter '*.csv' -File|Sort-Object Name)
 $csv='phase,operation,offset,bytes,latency_ms,error,correct'+"`n"
 foreach($file in $csvs){foreach($line in [IO.File]::ReadLines($file.FullName)|Select-Object -Skip 1){$csv+=$file.BaseName+','+$line+"`n"}}
 [IO.File]::WriteAllText((Join-Path $share samples.csv),$csv,[Text.UTF8Encoding]::new($false))
 $hashes=@(Get-ChildItem -LiteralPath $share -Recurse -File|Where-Object Name -ne 'manifest.json'|ForEach-Object {[ordered]@{path=$_.FullName.Substring($share.Length+1).Replace('\','/');sha256=(Get-FileHash -LiteralPath $_.FullName).Hash}})
 [IO.File]::WriteAllText((Join-Path $share manifest.json),($hashes|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))
 Add-Type -AssemblyName System.IO.Compression.FileSystem
 $export=Join-Path $output export;[void][IO.Directory]::CreateDirectory($export)
 $zip=Join-Path $export ('SuperHDD-benchmark-'+$run.id+'.zip');if(Test-Path -LiteralPath $zip){[IO.File]::Delete($zip)}
 [IO.Compression.ZipFile]::CreateFromDirectory($share,$zip)
 return $public
}
