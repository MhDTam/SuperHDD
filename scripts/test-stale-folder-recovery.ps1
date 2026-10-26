$ErrorActionPreference='Stop'
$workspace=Split-Path $PSScriptRoot -Parent
$run=[Guid]::NewGuid().ToString('N');$fixture='D:\SuperHDD-ConfigLab-'+$run;$source=Join-Path $fixture selected
$sha=[Security.Cryptography.SHA256]::Create()
try{$appId=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}
$runtime=Join-Path $workspace ('artifacts\library-runtime\'+$appId);[void][IO.Directory]::CreateDirectory($runtime)
$output=Join-Path $workspace ('artifacts\recovery-tests\'+$run);[void][IO.Directory]::CreateDirectory($output)
$current=Join-Path $runtime current.json;$launcher=Join-Path $workspace 'tools\loading-cache\schedule-session.ps1';$helper=Join-Path $workspace 'tools\loading-cache\recover-stale.ps1'
$reference=Get-Content -Raw (Join-Path $workspace 'artifacts\library-runtime\9be1bad80cbe5c8cebff4c1e977df6e5\current.json')|ConvertFrom-Json
$config=[Guid]::NewGuid().ToString('N');$bytes=New-Object byte[] 131072;[Random]::new(1943).NextBytes($bytes)
[void][IO.Directory]::CreateDirectory($source);[IO.File]::WriteAllBytes((Join-Path $source asset.bin),$bytes)
$expected=(Get-FileHash -LiteralPath (Join-Path $source asset.bin)).Hash
$request=@{sourceDiskId=$reference.sourceDiskId;sourceLetter='D:';ssdDiskId=$reference.cacheDiskId;ssdLetter='M:';capacityBytes=33554432;scope=@{mode='selected';folders=@($source)}}
@{request=$request;configurationId=$config;sourceVolumeId=$reference.sourceVolumeId;ssdVolumeId=$reference.cacheVolumeId}|ConvertTo-Json -Depth 6|Set-Content -Encoding utf8 (Join-Path $runtime session-plan.json)
function Journal{Get-Content -Raw $current|ConvertFrom-Json}
function Wait-For([scriptblock]$condition){for($i=0;$i -lt 300;$i++){if(& $condition){return};Start-Sleep -Milliseconds 100};throw 'Recovery test timeout'}
$proof=[ordered]@{runId=$run;appId=$appId;passed=$false;checks=@{};error=$null}
try{
 & $launcher -Action start -AppId $appId -Source $source -CapacityBytes 33554432 -Lab|Out-Null
 Wait-For {(Test-Path $current) -and (Journal).phase -eq 'active'}
 & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $helper -AppId $appId|Out-Null
 if($LASTEXITCODE -or (Journal).restored -or (Journal).phase -ne 'active'){throw 'Live session changed'}
 $proof.checks.liveSessionPreserved=$true
 $j=Journal;$owned=Get-Process -Id $j.supervisorPid
 if($owned.StartTime.ToUniversalTime().Ticks -ne $j.supervisorStartTicks){throw 'Owned supervisor identity mismatch'}
 $owned.Kill();[void]$owned.WaitForExit(10000)
 Wait-For {-not(Get-Process -Id $j.workerPid -ErrorAction SilentlyContinue)}
 $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$digest=[Security.Cryptography.SHA256]::Create()
 try{$key=([BitConverter]::ToString($digest.ComputeHash([Text.Encoding]::UTF8.GetBytes($workspace+'|'+$sid)))).Replace('-','').Substring(0,16)}finally{$digest.Dispose()}
 $service=New-Object -ComObject 'Schedule.Service';$service.Connect();$task=$service.GetFolder('\').GetTask('SuperHDD-Recovery-'+$key)
 if($task.Definition.RegistrationInfo.Description -ne ('Super HDD stale-folder recovery: '+$workspace+' | '+$sid) -or $task.Definition.Principal.RunLevel -ne 0 -or $task.Definition.Triggers.Item(1).Type -ne 9){throw 'Recovery task identity or logon trigger mismatch'}
 [void]$task.Run($null);Wait-For {(Journal).restored};Wait-For {$task.State -ne 4}
 if((Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Broken mount remains'}
 if((Get-FileHash (Join-Path $source asset.bin)).Hash -ne $expected){throw 'Restored data changed'}
 $proof.checks.registeredLogonActionRestoresOrphan=$true;$proof.checks.originalBytesPreserved=$true
 $saved=[IO.File]::ReadAllBytes($current)
 try{
  $bad=Journal;$bad.restored=$false;$bad.sourceIdentity='00000000:0000000000000000';$bad|ConvertTo-Json -Depth 10|Set-Content -Encoding utf8 $current
  # Windows PowerShell 5.1 turns redirected native stderr into a terminating error under Stop.
  & {$ErrorActionPreference='Continue';& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $helper -AppId $appId 2> (Join-Path $output expected-refusal.log)|Out-Null}
  if($LASTEXITCODE -eq 0 -or (Journal).restored){throw 'Wrong source identity accepted'}
  if((Get-FileHash (Join-Path $source asset.bin)).Hash -ne $expected){throw 'Refusal changed data'}
  $proof.checks.mismatchedIdentityPreserved=$true
 }finally{[IO.File]::WriteAllBytes($current,$saved)}
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 try{if((Test-Path $current) -and -not (Journal).restored){$j=Journal;[IO.File]::WriteAllText((Join-Path $j.task stop),'stop');Wait-For {(Journal).restored}}}catch{$proof['cleanupError']=$_.Exception.Message}
 if((Test-Path $current) -and (Journal).restored){
  Start-Sleep -Milliseconds 700
  try{& $launcher -Action remove -AppId $appId|Out-Null}catch{$proof['taskCleanupError']=$_.Exception.Message}
  if([IO.Path]::GetFullPath($fixture) -ne ('D:\SuperHDD-ConfigLab-'+$run)){throw 'Fixture boundary changed'}
  [IO.File]::Delete((Join-Path $source asset.bin));[IO.Directory]::Delete($source);[IO.Directory]::Delete($fixture)
  $pool=Join-Path ('M:\SuperHDD-Cache\'+$reference.cacheOwner+'\pools') $config
  if([IO.Path]::GetFileName($pool) -ne $config){throw 'Pool boundary changed'}
  foreach($leaf in @('cache.bin','cache.bin.index')){if(Test-Path -LiteralPath (Join-Path $pool $leaf)){[IO.File]::Delete((Join-Path $pool $leaf))}}
  if(Test-Path -LiteralPath $pool){[IO.Directory]::Delete($pool)}
 }
 $proof|ConvertTo-Json -Depth 7|Set-Content -Encoding utf8 (Join-Path $output result.json)
}
$proof|ConvertTo-Json -Depth 7
if(-not $proof.passed){throw 'Stale folder recovery test failed'}
