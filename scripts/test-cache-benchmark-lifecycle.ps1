param([Parameter(Mandatory=$true)][ValidateSet('cancel','crash')][string]$Mode,[Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{32}$')][string]$ConfigurationId)
$ErrorActionPreference='Stop'
$workspace=Split-Path $PSScriptRoot -Parent
$client=Join-Path $workspace 'target\debug\examples\benchmark_client.exe'
$pointer=Join-Path $workspace 'artifacts\cache-benchmark\current.json'
$before=if(Test-Path $pointer){(Get-Content -Raw $pointer|ConvertFrom-Json).runId}else{''}
$log=Join-Path $workspace ('artifacts\cache-benchmark\lifecycle-'+[Guid]::NewGuid().ToString('N'))
$launch=Start-Process -FilePath $client -ArgumentList @('start',$ConfigurationId,'quick','mixed') -PassThru -WindowStyle Hidden -RedirectStandardOutput ($log+'.json') -RedirectStandardError ($log+'.err')
$deadline=[DateTime]::UtcNow.AddSeconds(40)
do{Start-Sleep -Milliseconds 100;$id=if(Test-Path $pointer){(Get-Content -Raw $pointer|ConvertFrom-Json).runId}else{''};if([DateTime]::UtcNow -gt $deadline){throw ('Start timeout: '+[IO.File]::ReadAllText($log+'.err'))}}while(-not $id -or $id -eq $before)
if($id -notmatch '^[a-f0-9]{32}$'){throw 'Missing run identity'}
$output=Join-Path $workspace ('artifacts\cache-benchmark\runs\'+$id)
$deadline=[DateTime]::UtcNow.AddSeconds(50)
do{
 Start-Sleep -Milliseconds 100
 $state=Get-Content -LiteralPath (Join-Path $output state.json) -Raw|ConvertFrom-Json
 if([DateTime]::UtcNow -gt $deadline){throw 'Fixture startup timeout'}
}while($state.phase -notin @('firstPass','warming','measuring','failed','complete'))
if($state.phase -in @('failed','complete')){throw 'Did not reach an active phase'}
if($Mode -eq 'cancel'){
 & $client cancel|Out-Null
 if($LASTEXITCODE){throw 'Cancel rejected'}
}else{
 $identity=Get-Content -LiteralPath (Join-Path $output runner.json) -Raw|ConvertFrom-Json
 $owned=Get-Process -Id $identity.pid
 $cim=Get-CimInstance Win32_Process -Filter ('ProcessId='+$identity.pid)
 if($owned.StartTime.ToUniversalTime().ToFileTimeUtc() -ne $identity.createdFileTime -or $cim.CommandLine -notlike ('*run-cache-benchmark.ps1*'+$id+'*')){throw 'Owned runner identity mismatch'}
 $owned.Kill();[void]$owned.WaitForExit(10000)
 $journal=Get-Content -LiteralPath (Join-Path $output fixture-journal.json) -Raw|ConvertFrom-Json
 $deadline=[DateTime]::UtcNow.AddSeconds(35)
 while($true){$p=Get-Process -Id $journal.workerPid -ErrorAction SilentlyContinue;if(-not $p -or $p.StartTime.ToUniversalTime().ToFileTimeUtc() -ne $journal.workerCreatedFileTime){break};if([DateTime]::UtcNow -gt $deadline){throw 'Orphan worker did not exit'};Start-Sleep -Milliseconds 200}
 $interrupted=(& $client status)|ConvertFrom-Json
 if(-not $interrupted.canRecover -or $interrupted.phase -ne 'interrupted'){throw 'Interrupted state not detected'}
 & $client recover|Out-Null
 if($LASTEXITCODE){throw 'Recovery rejected'}
}
$deadline=[DateTime]::UtcNow.AddSeconds(50)
do{Start-Sleep -Milliseconds 200;$state=Get-Content -Raw (Join-Path $output state.json)|ConvertFrom-Json;if([DateTime]::UtcNow -gt $deadline){throw 'Cleanup timeout'}}while($state.phase -notin @('cancelled','recovered','failed'))
$result=Get-Content -Raw (Join-Path $output internal-result.json)|ConvertFrom-Json
$report=Get-Content -Raw (Join-Path $output share/report.json)|ConvertFrom-Json
if(-not $result.cleanupComplete -or $report.valid -or $report.publicationReady){throw 'Cleanup or invalid-result contract failed'}
$request=Get-Content -Raw (Join-Path $output request.json)|ConvertFrom-Json
foreach($letter in @($request.sourceLetter,$request.ssdLetter)){if(Test-Path -LiteralPath ($letter+'\SuperHDD-Benchmark-'+$id)){throw 'Fixture remains'}}
[ordered]@{passed=$true;mode=$Mode;runId=$id;phase=$state.phase;cleanupComplete=$true;invalidReportRetained=$true}|ConvertTo-Json|Set-Content (Join-Path $output lifecycle-test.json)
Get-Content -Raw (Join-Path $output lifecycle-test.json)
