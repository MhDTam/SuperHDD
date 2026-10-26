# Keep the requesting process alive and record actual process creation, rather
# than treating the void Shell.Application.ShellExecute return as approval.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$record=Join-Path $root 'artifacts\local-native-launch.json'
$state=[ordered]@{at=[DateTime]::UtcNow.ToString('o');phase='requestingWindowsConsent';requesterPid=$PID;elevatedPid=$null;exitCode=$null;error=$null}
function Save {$state|ConvertTo-Json|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
Save
try{
    $script=Join-Path $PSScriptRoot 'complete-local-native-cache.ps1'
    $engine=Join-Path $PSHOME 'pwsh.exe'
    $process=Start-Process -FilePath $engine -ArgumentList ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$script+'"') -WorkingDirectory $root -Verb RunAs -WindowStyle Hidden -PassThru
    $state.elevatedPid=$process.Id;$state.phase='running';Save
    $process.WaitForExit();$state.exitCode=$process.ExitCode;$state.phase=if($process.ExitCode -eq 0){'finished'}else{'failed'};Save
}catch{$state.phase='notStarted';$state.error=$_.Exception.Message;Save;throw}
