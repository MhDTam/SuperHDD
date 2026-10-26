param([switch]$RegisterLogon,[switch]$RegisterOnly,[switch]$Resume,[ValidatePattern('^[a-f0-9]{32}$')][string]$AppId)
$ErrorActionPreference='Stop'
if($PSVersionTable.PSVersion.Major -le 5){$env:PSModulePath=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'}
$workspace=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$runtime=Join-Path $workspace 'artifacts\library-runtime'
[void][IO.Directory]::CreateDirectory($runtime)
if($RegisterLogon){
 $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
 $digest=[Security.Cryptography.SHA256]::Create()
 try{$key=([BitConverter]::ToString($digest.ComputeHash([Text.Encoding]::UTF8.GetBytes($workspace+'|'+$sid)))).Replace('-','').Substring(0,16)}finally{$digest.Dispose()}
 $name='SuperHDD-Recovery-'+$key;$owner='Super HDD stale-folder recovery: '+$workspace+' | '+$sid
 $service=New-Object -ComObject 'Schedule.Service';$service.Connect();$folder=$service.GetFolder('\')
 $existing=$null
 try{$existing=$folder.GetTask($name)}catch{if($_.Exception.HResult -ne -2147024894){throw}}
 if($existing -and $existing.Definition.RegistrationInfo.Description -ne $owner){throw 'Foreign recovery task preserved'}
 $definition=$service.NewTask(0);$definition.RegistrationInfo.Description=$owner
 $definition.Principal.UserId=$sid;$definition.Principal.LogonType=3;$definition.Principal.RunLevel=0
 $definition.Settings.Enabled=$true;$definition.Settings.AllowDemandStart=$true
 $definition.Settings.StartWhenAvailable=$true;$definition.Settings.Hidden=$true
 $definition.Settings.DisallowStartIfOnBatteries=$false;$definition.Settings.StopIfGoingOnBatteries=$false
 $definition.Settings.ExecutionTimeLimit='PT5M';$definition.Settings.MultipleInstances=2
 $trigger=$definition.Triggers.Create(9);$trigger.UserId=$sid;$trigger.Enabled=$true
 $exec=$definition.Actions.Create(0);$exec.WorkingDirectory=$workspace
 $exec.Path=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
 # At logon, restore interrupted folders and then resume caches that were
 # active at shutdown. A user Stop removes the resume marker.
 $exec.Arguments='-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "'+$PSCommandPath+'" -Resume'
 [void]$folder.RegisterTaskDefinition($name,$definition,6,$sid,$null,3,$null)
}
if($RegisterOnly){return}
$lock=$null
try{$lock=[IO.File]::Open((Join-Path $runtime recovery.lock),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)}catch{throw 'A recovery scan is already running'}
function Process-IsAlive($identity,$ticks){
 if(-not $identity -or -not $ticks){return $false}
 $process=Get-Process -Id $identity -ErrorAction SilentlyContinue
 if(-not $process){return $false}
 # Failure to read creation time is an error, never proof that a process died.
 return $process.StartTime.ToUniversalTime().Ticks -eq $ticks
}
$results=@()
try{
 $qualified=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\qualified.json') -Raw|ConvertFrom-Json
 $session=Join-Path $PSScriptRoot session.ps1
 if((Get-FileHash -LiteralPath $session).Hash -ne $qualified.sessionSha256){throw 'Qualified recovery source mismatch'}
 foreach($directory in @(Get-ChildItem -LiteralPath $runtime -Directory)){
  if($directory.Name -notmatch '^[a-f0-9]{32}$' -or ($AppId -and $directory.Name -ne $AppId)){continue}
  if($directory.Attributes -band [IO.FileAttributes]::ReparsePoint){continue}
  $current=Join-Path $directory.FullName current.json
  if(-not (Test-Path -LiteralPath $current)){continue}
  try{
   if((Get-Item -LiteralPath $current).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked journal preserved'}
   $record=Get-Content -LiteralPath $current -Raw|ConvertFrom-Json
   if($record.restored -eq $true){continue}
   if((Process-IsAlive $record.supervisorPid $record.supervisorStartTicks) -or (Process-IsAlive $record.workerPid $record.workerStartTicks)){
    $results+=@{appId=$directory.Name;status='live-preserved'};continue
   }
   $psi=New-Object Diagnostics.ProcessStartInfo
   $psi.FileName=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
   $psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true;$psi.RedirectStandardOutput=$true
   $psi.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$session+'" -Action recover -AppId '+$directory.Name
   $child=[Diagnostics.Process]::Start($psi);$stdout=$child.StandardOutput.ReadToEndAsync();$stderr=$child.StandardError.ReadToEndAsync()
   # Never kill a recovery process halfway through a filesystem transaction.
   $child.WaitForExit();$errorText=$stderr.GetAwaiter().GetResult();$exit=$child.ExitCode;$child.Dispose()
   $after=Get-Content -LiteralPath $current -Raw|ConvertFrom-Json
   if($exit -ne 0 -or -not $after.restored){throw ('Recovery preserved files for inspection: '+$errorText)}
   $results+=@{appId=$directory.Name;status='restored'}
  }catch{$results+=@{appId=$directory.Name;status='preserved';error=$_.Exception.Message}}
 }
 if($Resume){
  # Only caches that were active at shutdown carry a marker. Each start runs
  # the full session checks (qualification, device identity, game closed).
  $library=$null;try{$library=Get-Content -LiteralPath (Join-Path $runtime library.json) -Raw|ConvertFrom-Json}catch{}
  $launcher=Join-Path $PSScriptRoot schedule-session.ps1
  foreach($directory in @(Get-ChildItem -LiteralPath $runtime -Directory)){
   if($directory.Name -notmatch '^[a-f0-9]{32}$' -or ($AppId -and $directory.Name -ne $AppId) -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)){continue}
   if(-not (Test-Path -LiteralPath (Join-Path $directory.FullName 'resume-at-logon'))){continue}
   try{
    $record=Get-Content -LiteralPath (Join-Path $directory.FullName current.json) -Raw|ConvertFrom-Json
    if($record.restored -ne $true){throw 'Folder was not restored'}
    if((Process-IsAlive $record.supervisorPid $record.supervisorStartTicks) -or (Process-IsAlive $record.workerPid $record.workerStartTicks)){continue}
    $plan=Get-Content -LiteralPath (Join-Path $directory.FullName session-plan.json) -Raw|ConvertFrom-Json
    $source=[string]@($plan.request.scope.folders)[0]
    $config=@($library.configurations|Where-Object {$_.id -eq $plan.configurationId})[0]
    if(-not $config -or [UInt64]$config.budgetBytes -ne [UInt64]$plan.request.capacityBytes -or $config.sourceLetter -ne $plan.request.sourceLetter -or $config.ssdLetter -ne $plan.request.ssdLetter){throw 'Configuration changed since the cache was active'}
    if(-not @($library.apps|Where-Object {$_.id -eq $directory.Name -and $_.path -eq $source}).Count){throw 'Profile no longer matches'}
    & $launcher -Action start -AppId $directory.Name -Source $source -CapacityBytes ([UInt64]$plan.request.capacityBytes)|Out-Null
    $results+=@{appId=$directory.Name;status='resumed'}
   }catch{$results+=@{appId=$directory.Name;status='resume-skipped';error=$_.Exception.Message}}
  }
 }
 $report=[ordered]@{atUtc=[DateTime]::UtcNow.ToString('o');sessions=$results;startsCache=[bool]@($results|Where-Object {$_.status -eq 'resumed'}).Count;changesGameContents=$false}
 [IO.File]::WriteAllText((Join-Path $runtime last-recovery.json),($report|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 $report|ConvertTo-Json -Depth 6
 if(@($results|Where-Object status -eq 'preserved').Count){throw 'Some stale folders require inspection; see last-recovery.json'}
}finally{$lock.Dispose()}
