$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$binary=Join-Path $root 'target\release\SuperHDD.exe'
$node='F:\Important Programs\Node.js\node.exe'
$stamp=[DateTime]::UtcNow.ToString('yyyyMMdd')
if(Get-Process SuperHDD -ErrorAction SilentlyContinue){throw 'Existing desktop process; will not replace it.'}
if(Get-NetTCPConnection -State Listen -LocalPort 9223 -ErrorAction SilentlyContinue){throw 'Debug port already in use.'}
if($env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS){throw 'Existing WebView2 debug environment; will not replace it.'}
$owned=$null
try{
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9223 --remote-debugging-address=127.0.0.1'
    try{$owned=Start-Process -FilePath $binary -WindowStyle Hidden -PassThru}finally{Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS}
    for($attempt=0;$attempt -lt 120;$attempt++){
        if($owned.HasExited){throw 'Native application exited before audit.'}
        try{$null=Invoke-RestMethod http://127.0.0.1:9223/json/version -TimeoutSec 1;break}catch{Start-Sleep -Milliseconds 250}
    }
    & $node (Join-Path $root 'scripts\test-native-ui.cjs')
    if($LASTEXITCODE){throw 'Actual native UI audit failed.'}
}finally{
    if($owned){$owned.Refresh();if(-not $owned.HasExited){Stop-Process -Id $owned.Id;$null=$owned.WaitForExit(10000)}}
}
for($attempt=0;$attempt -lt 40;$attempt++){
    if(-not(Get-NetTCPConnection -State Listen -LocalPort 9223 -ErrorAction SilentlyContinue)){break}
    Start-Sleep -Milliseconds 250
}
$debugPortClosed=-not(Get-NetTCPConnection -State Listen -LocalPort 9223 -ErrorAction SilentlyContinue)
if(-not $debugPortClosed){throw 'Debug port remained open after audit.'}
$owned=Start-Process -FilePath $binary -WindowStyle Hidden -PassThru
try{
    $null=$owned.WaitForInputIdle(15000)
    Start-Sleep -Seconds 2
    $owned.Refresh()
    if($owned.HasExited -or $owned.MainWindowTitle -ne 'Super HDD'){throw 'Normal startup failed.'}
    $startup=[ordered]@{at=[DateTime]::UtcNow.ToString('o');exe=$binary;sha256=(Get-FileHash -LiteralPath $binary).Hash;pid=$owned.Id;title=$owned.MainWindowTitle;running=$true;debugPortClosed=$debugPortClosed;debugEnvironmentAbsent=(-not $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS)}
    $startup|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $root ('artifacts\runtime\native-final-startup-'+$stamp+'.json')) -Encoding utf8
    $second=Start-Process -FilePath $binary -WindowStyle Hidden -PassThru
    if(-not $second.WaitForExit(10000)){Stop-Process -Id $second.Id;throw 'Second instance did not exit.'}
    $instances=@(Get-CimInstance Win32_Process -Filter "Name='SuperHDD.exe'")
    if($second.ExitCode -ne 0 -or $instances.Count -ne 1 -or $instances[0].ProcessId -ne $owned.Id){throw 'Single-instance check failed.'}
    [ordered]@{at=[DateTime]::UtcNow.ToString('o');firstPid=$owned.Id;secondExitCode=$second.ExitCode;remainingInstances=$instances.Count;passed=$true}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $root ('artifacts\runtime\single-instance-'+$stamp+'.json')) -Encoding utf8
    $startup|ConvertTo-Json
}finally{
    $owned.Refresh();if(-not $owned.HasExited){Stop-Process -Id $owned.Id;$null=$owned.WaitForExit(10000)}
}
