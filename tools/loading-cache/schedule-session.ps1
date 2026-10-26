param([ValidateSet('start','recover','remove')][string]$Action='start',[Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{32}$')][string]$AppId,[string]$Source,[UInt64]$CapacityBytes=24999952384,[switch]$Lab)
$ErrorActionPreference='Stop'
# Task Scheduler starts the host outside the command runner's lifetime/job.
# This is an on-demand task under the logged-on user, with no stored password.
$workspace=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$digest=[Security.Cryptography.SHA256]::Create()
try{$key=([BitConverter]::ToString($digest.ComputeHash([Text.Encoding]::UTF8.GetBytes($workspace+'|'+$sid)))).Replace('-','').Substring(0,16)}finally{$digest.Dispose()}
$name='SuperHDD-Library-'+$key+'-'+$AppId
$owner='Super HDD owned local folder host: '+$workspace+' | '+$sid
$service=New-Object -ComObject 'Schedule.Service';$service.Connect();$folder=$service.GetFolder('\')
$existing=$null
try{$existing=$folder.GetTask($name)}catch{if($_.Exception.HResult -ne -2147024894){throw}}
if($existing){
 if($existing.Definition.RegistrationInfo.Description -ne $owner){throw 'Foreign task with the same name; preserved'}
 if($existing.State -eq 4){throw 'The owned task is already running'}
}
if($Action -eq 'remove'){if($existing){$folder.DeleteTask($name,0)};return}
$definition=$service.NewTask(0)
$definition.RegistrationInfo.Description=$owner
$definition.Principal.UserId=$sid;$definition.Principal.LogonType=3;$definition.Principal.RunLevel=0
$definition.Settings.Enabled=$true;$definition.Settings.AllowDemandStart=$true
$definition.Settings.DisallowStartIfOnBatteries=$false;$definition.Settings.StopIfGoingOnBatteries=$false
$definition.Settings.ExecutionTimeLimit='PT0S';$definition.Settings.MultipleInstances=2
$definition.Settings.Hidden=$true
$exec=$definition.Actions.Create(0)
$exec.WorkingDirectory=$workspace
if($Action -ne 'remove'){
 $exec.Path=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
 $arguments=@('-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',(Join-Path $PSScriptRoot 'session.ps1'),'-Action',$Action,'-AppId',$AppId)
 if($Action -eq 'start'){
  if(-not $Source -or $Source -match '[\r\n"]'){throw 'Invalid source'}
  $arguments+=@('-Source',$Source,'-CapacityBytes',[string]$CapacityBytes)
  if($Lab){$arguments+='-Lab'}
 }
 $exec.Arguments=($arguments|ForEach-Object {'"'+$_+'"'}) -join ' '
}
$task=$folder.RegisterTaskDefinition($name,$definition,6,$sid,$null,3,$null)
[void]$task.Run($null)
# Return registration promptly. Readiness is checked using the durable session
# journal by the native caller, not by the scheduled-task launch result.
[pscustomobject]@{taskName=$name;action=$Action;userSid=$sid;runLevel='limited';onDemandOnly=$true}|ConvertTo-Json -Compress
