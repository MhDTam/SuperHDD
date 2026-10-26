# Separate user-mode processes and owned files only; no Windows initiator or OS disk.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$run)
$record=Join-Path $root 'artifacts\iscsi-process-recovery-latest.json'
$exe=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
$source='D:\SuperHDD-iSCSI-'+$run+'.img'
$caches=@((Join-Path $runRoot 'ssd-cache.bin'),(Join-Path $runRoot 'ssd-cache-recovered.bin'))
if(Get-NetTCPConnection -State Listen -LocalPort 33260 -ErrorAction SilentlyContinue){throw 'Private test port is in use.'}
$hdd=Get-Partition -DriveLetter D|Get-Disk
$ssd=Get-Partition -DriveLetter M|Get-Disk
if($hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $hdd.IsBoot -or $hdd.IsSystem -or $ssd.FriendlyName -notmatch '980 PRO' -or $hdd.UniqueId -eq $ssd.UniqueId){throw 'Expected D HDD and M SSD unavailable.'}
if((Get-Volume D).SizeRemaining -lt 1GB -or (Get-Volume M).SizeRemaining -lt 1GB){throw 'Insufficient space for disposable files.'}
$boot=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$service=Get-Service MSiSCSI
$priorStatus=[string]$service.Status;$priorStart=[string]$service.StartType
New-Item -ItemType Directory -Path $runRoot|Out-Null
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;passed=$false;phase='starting';scope='User-mode process interruption and full payload recovery only; no Windows initiator or existing D interception';error=$null;bootBefore=$boot;sourcePath=$source}
$result.binaries=@('iscsi-probe.exe','superhdd_iscsi_probe.dll','ISCSI.dll','DiskAccessLibrary.dll'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path (Split-Path $exe) $_)|Select-Object Path,Hash})
$result.sourceHashes=@('crates\cache-core\src\lib.rs','tools\iscsi-cache-probe\src\lib.rs','tools\iscsi-cache-probe\Probe.cs','tools\iscsi-cache-probe\Protocol.cs','scripts\test-iscsi-process-recovery.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
function Save {$result|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
function Wait-For([scriptblock]$action,[int]$seconds=20){
    $deadline=[DateTime]::UtcNow.AddSeconds($seconds)
    do{$value=& $action;if($value){return $value};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
    throw 'Timed out waiting for owned recovery process.'
}
function Control([int]$operation){
    $token=[Guid]::NewGuid().ToString('N')
    Set-Content -LiteralPath (Join-Path $runRoot 'command.tmp') -Value ($token+' '+$operation) -NoNewline
    Move-Item -LiteralPath (Join-Path $runRoot 'command.tmp') -Destination (Join-Path $runRoot 'command')
    $null=Wait-For {if(Test-Path -LiteralPath (Join-Path $runRoot 'ack')){(Get-Content -LiteralPath (Join-Path $runRoot 'ack') -Raw) -eq ($token+' 0')}}
    if($operation -ne 5){Get-Content -LiteralPath (Join-Path $runRoot 'stats.json') -Raw|ConvertFrom-Json}
}
function New-Target([switch]$Reopen){
    $mode=if($Reopen){'server-reopen'}else{'server'}
    Start-Process -FilePath $exe -ArgumentList @($mode,$run) -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot ($mode+'.log')) -RedirectStandardError (Join-Path $runRoot ($mode+'-error.log')) -PassThru
}
function New-Client([string]$mode,[string]$tag){
    Start-Process -FilePath $exe -ArgumentList @('protocol-client',$mode,$run) -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot ($tag+'.log')) -RedirectStandardError (Join-Path $runRoot ($tag+'-error.log')) -PassThru
}
function Client([string]$mode,[string]$tag){
    $client=New-Client $mode $tag
    if(-not $client.WaitForExit(60000)){$client.Kill();$null=$client.WaitForExit(5000);throw ('Owned client timed out: '+$tag)}
    if($client.ExitCode){throw ('Owned client failed: '+$tag)}
    Get-Content -LiteralPath (Join-Path $runRoot ($mode+'.json')) -Raw|ConvertFrom-Json
}
$target=$null;$held=$null;$lock=$null
try{
    Save
    $lock=[IO.File]::Open((Join-Path $root 'artifacts\runtime\kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    $target=New-Target
    $null=Wait-For {if($target.HasExited){throw 'Owned target exited before ready.'};Test-Path -LiteralPath (Join-Path $runRoot 'ready')}
    $result.seed=Client seed seed
    $held=New-Client mutate-hold mutation
    $null=Wait-For {if($held.HasExited){throw 'Mutation client exited early.'};Test-Path -LiteralPath (Join-Path $runRoot 'mutate-hold.json')}
    $result.mutation=Get-Content -LiteralPath (Join-Path $runRoot 'mutate-hold.json') -Raw|ConvertFrom-Json
    if(-not $result.mutation.passed -or $held.HasExited){throw 'No acknowledged mutation with a held connection.'}
    $result.phase='interrupting-connected-target';Save
    $result.interruptedPid=$target.Id
    $target.Kill();if(-not $target.WaitForExit(5000)){throw 'Owned target did not exit.'}
    Set-Content -LiteralPath (Join-Path $runRoot 'release-client') -Value 'release' -NoNewline
    if(-not $held.WaitForExit(10000) -or $held.ExitCode){throw 'Held client did not finish after interruption.'}
    # The abandoned cache is deliberately damaged: recovered reads must use the
    # authoritative source and a new mapping/cache file, never this old payload.
    $oldCache=[IO.File]::Open($caches[0],[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$damaged=[byte[]]::new(65536);$oldCache.Write($damaged,0,$damaged.Length);$oldCache.Flush($true)}finally{$oldCache.Dispose()}
    $result.abandonedCacheDamaged=$true
    Remove-Item -LiteralPath (Join-Path $runRoot 'ready')
    $target=New-Target -Reopen
    $null=Wait-For {if($target.HasExited){throw 'Owned target reopen failed.'};Test-Path -LiteralPath (Join-Path $runRoot 'ready')}
    $result.recoveryStart=Control 0
    if(-not $result.recoveryStart.recovered -or $result.recoveryStart.cacheMappingsReused -or $result.recoveryStart.occupancyBytes -ne 0){throw 'Source recovery did not begin with an empty cache.'}
    $result.phase='checking-complete-durable-payload';Save
    $result.cold=Client read-all-mutated cold
    $cold=Control 0
    $result.warm=Client read-all-mutated warm
    $warm=Control 0
    $result.warmDelta=[ordered]@{hits=$warm.statistics.hits-$cold.statistics.hits;ssdReadBytes=$warm.statistics.ssdReadBytes-$cold.statistics.ssdReadBytes;sourceReadBytes=$warm.statistics.sourceReadBytes-$cold.statistics.sourceReadBytes}
    if($result.warmDelta.hits -ne 256 -or $result.warmDelta.ssdReadBytes -ne 16MB -or $result.warmDelta.sourceReadBytes -ne 0){throw 'Recovered payload warm reads did not stay in SSD.'}
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    $result.phase='cleanup';Save
    try{
        if($held -and -not $held.HasExited){Set-Content -LiteralPath (Join-Path $runRoot 'release-client') -Value 'release' -NoNewline;if(-not $held.WaitForExit(10000)){$held.Kill();$null=$held.WaitForExit(5000)}}
        if($target -and -not $target.HasExited){$null=Control 5;if(-not $target.WaitForExit(10000)){throw 'Owned target did not stop.'}}
        if($target -and $target.HasExited -and (Test-Path -LiteralPath (Join-Path $runRoot 'source-owner.json'))){
            if(Test-Path -LiteralPath $source){$result.sourceSha256=(Get-FileHash -LiteralPath $source).Hash}
            foreach($path in @($source)+$caches){if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}}
        }
    }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
    $result.ownedFilesRemoved=(-not(Test-Path -LiteralPath $source) -and @($caches|Where-Object{Test-Path -LiteralPath $_}).Count -eq 0)
    $result.portClosed=-not(Get-NetTCPConnection -State Listen -LocalPort 33260 -ErrorAction SilentlyContinue)
    $now=Get-Service MSiSCSI
    $result.initiatorUnchanged=([string]$now.Status -eq $priorStatus -and [string]$now.StartType -eq $priorStart)
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootAfter=$after
    $result.bootUnchanged=($after.machineSha256 -eq $boot.machineSha256 -and $after.secureBootEnabled -eq $boot.secureBootEnabled -and $after.codeIntegrityOptions -eq $boot.codeIntegrityOptions)
    $result.passed=[bool]($result.passed -and $result.ownedFilesRemoved -and $result.portClosed -and $result.initiatorUnchanged -and $result.bootUnchanged)
    $result.phase='finished';Save
    if($lock){$lock.Dispose()}
}
[pscustomobject]@{passed=$result.passed;runId=$run;error=$result.error;warmDelta=$result.warmDelta;cleanup=$result.ownedFilesRemoved;record=$record}|ConvertTo-Json -Depth 5
if(-not $result.passed){throw 'User-mode process recovery proof failed.'}
