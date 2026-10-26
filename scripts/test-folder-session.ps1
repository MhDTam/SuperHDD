$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N');$fixture='D:\SuperHDD-SessionLab-'+$run
$source=Join-Path $fixture 'selected';$runtime=Join-Path $root 'artifacts\folder-runtime';$current=Join-Path $runtime 'current.json'
$controller=Join-Path $root 'tools\folder-cache\session.ps1'
$before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$supervisor=$null
$result=[ordered]@{runId=$run;passed=$false;source=$source;checks=@{};error=$null}
function State {try{return Get-Content -LiteralPath $current -Raw|ConvertFrom-Json}catch{return $null}}
function Wait-State([scriptblock]$predicate){for($i=0;$i -lt 200;$i++){$s=State;if($s -and (& $predicate $s)){return $s};if($supervisor -and $supervisor.HasExited){throw ('Supervisor exited: '+$stderr.GetAwaiter().GetResult())};Start-Sleep -Milliseconds 100};throw 'Session phase timeout'}
function Launch {
    $psi=[Diagnostics.ProcessStartInfo]::new('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe')
    $psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true;$psi.RedirectStandardOutput=$true
    foreach($arg in @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$controller,'-Action','start','-Source',$source,'-CapacityBytes','24999952384','-Lab')){$psi.ArgumentList.Add($arg)}
    $script:supervisor=[Diagnostics.Process]::Start($psi);$script:stderr=$supervisor.StandardError.ReadToEndAsync();$script:stdout=$supervisor.StandardOutput.ReadToEndAsync()
    return Wait-State {param($s)$s.source -eq $source -and $s.phase -eq 'active' -and $s.supervisorPid -eq $supervisor.Id}
}
New-Item -ItemType Directory -Path $source|Out-Null
[IO.File]::WriteAllText((Join-Path $source 'fixture.txt'),'before')
try {
    $s=Launch
    if([IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'before'){throw 'Original visible path failed'}
    [IO.File]::WriteAllText((Join-Path $source 'fixture.txt'),'after')
    $worker=Get-Process -Id $s.workerPid
    if($worker.Path -ne (Join-Path $root 'artifacts\folder-cache\folder-worker.exe') -or $worker.StartTime.ToUniversalTime().Ticks -ne $s.workerStartTicks){throw 'Unexpected worker identity'}
    $worker.Kill();$worker.WaitForExit()
    $restored=Wait-State {param($s)$s.phase -eq 'stopped' -and $s.restored}
    if(-not $supervisor.WaitForExit(10000)){throw 'Supervisor did not finish recovery'}
    if([IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'after'){throw 'Crash recovery lost acknowledged write'}
    $result.checks.automaticWorkerCrashRestore=$true
    $result.crashSupervisorStderr=$stderr.GetAwaiter().GetResult()
    $s=Launch
    [IO.File]::WriteAllText((Join-Path $s.task 'stop'),'stop')
    if(-not $supervisor.WaitForExit(15000)){throw 'Normal stop timeout'}
    $stopped=Wait-State {param($s)$s.phase -eq 'stopped' -and $s.restored}
    if([IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'after'){throw 'Normal restore mismatch'}
    $result.checks.normalStopRestore=$true
    $result.normalSupervisorStderr=$stderr.GetAwaiter().GetResult()
    $s=Launch
    $supervisor.Kill();$supervisor.WaitForExit()
    $child=Get-Process -Id $s.workerPid -ErrorAction SilentlyContinue
    if($child -and -not $child.WaitForExit(10000)){throw 'Job did not close orphan worker'}
    $psi=[Diagnostics.ProcessStartInfo]::new('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe');$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true
    foreach($arg in @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$controller,'-Action','recover')){$psi.ArgumentList.Add($arg)}
    $recovery=[Diagnostics.Process]::Start($psi);$recoveryError=$recovery.StandardError.ReadToEndAsync()
    if(-not $recovery.WaitForExit(15000)){throw 'Recovery command timeout'}
    $result.recoveryStderr=$recoveryError.GetAwaiter().GetResult()
    $last=State
    if($last.phase -ne 'stopped' -or -not $last.restored -or [IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'after'){throw 'Supervisor crash recovery mismatch'}
    $result.checks.supervisorCrashJobAndRestartRecovery=$true
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    if($supervisor -and -not $supervisor.HasExited){$s=State;[IO.File]::WriteAllText((Join-Path $s.task 'stop'),'stop');[void]$supervisor.WaitForExit(15000)}
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootUnchanged=($before.codeIntegrityOptions -eq $after.codeIntegrityOptions -and $before.secureBootEnabled -eq $after.secureBootEnabled)
    $result.passed=[bool]($result.passed -and $result.bootUnchanged)
    $result.workerSha256=(Get-FileHash -LiteralPath (Join-Path $root 'artifacts\folder-cache\folder-worker.exe')).Hash
    $result.sessionSha256=(Get-FileHash -LiteralPath $controller).Hash
    $result|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $root 'artifacts\folder-cache\session-proof.json') -Encoding utf8
}
$result|ConvertTo-Json -Depth 7
if(-not $result.passed){throw 'Folder session supervisor proof failed'}
