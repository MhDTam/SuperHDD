# User-requested local D/M cache. The tested package gate must pass before any
# source handoff. Existing volumes are never formatted or repartitioned.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Start-Transcript -LiteralPath (Join-Path $root 'artifacts\physical-local-run.log') -Force|Out-Null
$record=Join-Path $root 'artifacts\physical-local-cache.json'
$result=[ordered]@{passed=$false;phase='preflight';physicalDAccelerationActive=$false;error=$null}
$seeded=$false;$active=$false;$configured=$false
function Save {$result|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
function Step([string]$phase){$result.phase=$phase;Save}
function Operation([string]$operation,$request=$null){
    $command=[ordered]@{operation=$operation;request=$request}|ConvertTo-Json -Depth 8 -Compress
    $encoded=[Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes($command))
    & $serviceExe --loopback-operation $encoded 2> (Join-Path $clientInfo.directory ('operation-'+$operation+'-error.log'))
    if($LASTEXITCODE){throw ('Native operation '+$operation+' failed with code '+$LASTEXITCODE)}
}
function Status([string]$kind='status'){
    $text=& $serviceExe ('--loopback-'+$kind) 2> (Join-Path $clientInfo.directory ($kind+'-error.log'))
    if($LASTEXITCODE){throw ('Native '+$kind+' is unavailable.')};$text|ConvertFrom-Json
}
function Client([string]$mode,[string]$tag){
    if((Get-FileHash -LiteralPath $clientInfo.client).Hash -ne $clientInfo.clientHash){throw 'Owned validation client changed.'}
    & $clientInfo.client $mode $tag 2> (Join-Path $clientInfo.directory ($tag+'-error.log'))
    if($LASTEXITCODE){throw ('Owned file oracle failed: '+$tag)}
    Get-Content -LiteralPath (Join-Path $clientInfo.directory ($tag+'.json')) -Raw|ConvertFrom-Json
}
try{
    $principal=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Windows administrator consent is required before this local setup.'}
    if(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json'){throw 'Existing owned cache state is preserved; recover it before fresh activation.'}
    $clientInfo=Get-Content -LiteralPath (Join-Path $root 'artifacts\physical-validation-client.json') -Raw|ConvertFrom-Json
    if($clientInfo.runId.Length -ne 32 -or $clientInfo.runId -notmatch '^[a-f0-9]{32}$' -or $clientInfo.fixture -ne ('D:\SuperHDD-native-validation-'+$clientInfo.runId+'.bin') -or $clientInfo.directory -ne (Join-Path $root ('artifacts\runtime\physical-validation-'+$clientInfo.runId)) -or $clientInfo.client -ne (Join-Path $clientInfo.directory 'PhysicalValidation.exe')){throw 'One fixed owned D validation file is required.'}
    if(Test-Path -LiteralPath $clientInfo.fixture){throw 'An existing validation path is preserved.'}
    $setup=Get-Content -LiteralPath (Join-Path $root 'artifacts\cache-25gb-setup.json') -Raw|ConvertFrom-Json
    if($setup.request.sourceLetter -ne 'D:' -or $setup.request.ssdLetter -ne 'M:' -or $setup.request.capacityBytes -ne 24999952384 -or $setup.request.scope){throw 'Only the reviewed D/M 25 GB whole-volume request is accepted.'}
    $source=Get-Partition -DriveLetter D|Get-Disk;$parent=Get-Partition -DriveLetter M|Get-Disk
    if($source.IsBoot -or $source.IsSystem -or $parent.IsBoot -or $parent.IsSystem -or $source.Number -eq $parent.Number -or $source.FriendlyName -notmatch 'HUA722020ALA331' -or $parent.FriendlyName -notmatch '980 PRO'){throw 'Expected separate local data HDD and SSD are absent.'}
    $before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    & (Join-Path $PSScriptRoot 'install-loopback-lab-package.ps1') -UpdateOwned -EnablePhysical
    $serviceExe=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'Super HDD Local Runtime\SuperHDDService.exe'
    $encoded=[Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes((([ordered]@{operation='start';request=$setup.request})|ConvertTo-Json -Depth 8 -Compress)))
    $preflight=& $serviceExe --loopback-preflight $encoded 2> (Join-Path $clientInfo.directory 'preflight-error.log')
    if($LASTEXITCODE){throw 'Fresh physical source/SSD checks failed.'};$result.preflight=$preflight|ConvertFrom-Json
    Step 'seed-one-new-owned-file'
    $result.seed=Client seed baseline;$seeded=$true;$result.baselineHash=(Get-FileHash -LiteralPath $clientInfo.fixture).Hash
    Step 'activate-reviewed-existing-D-volume'
    $configured=$true;Operation start $setup.request
    $state=Status
    if($state.phase -ne 'active' -or $state.nativeVersion -ne 6 -or $state.labOnly -or $state.sourceVolume -ne 'D:' -or $state.request.capacityBytes -ne 24999952384){throw 'Cache did not confirm the reviewed physical allocation.'}
    $active=$true;$result.runtime=$state
    if((Get-FileHash -LiteralPath $clientInfo.fixture).Hash -ne $result.baselineHash){throw 'Owned original fixture changed during handoff.'}
    Step 'verify-actual-ssd-cache-and-durable-write'
    $result.firstRead=Client read first;Start-Sleep -Milliseconds 1200;$cold=Status telemetry
    $result.warmRead=Client read warm;Start-Sleep -Milliseconds 1200;$warm=Status telemetry
    $result.warmDelta=[ordered]@{hits=$warm.hits-$cold.hits;ssdReadBytes=$warm.ssdReadBytes-$cold.ssdReadBytes;sourceReadBytes=$warm.sourceReadBytes-$cold.sourceReadBytes}
    if($result.warmDelta.hits -le 0 -or $result.warmDelta.ssdReadBytes -lt 6MB){throw 'Actual SSD cache reads were not confirmed.'}
    $result.mutation=Client mutate mutation
    Operation pause;$paused=Status telemetry
    $result.authoritativeFullOracle=Client read-all-mutated source-full
    Start-Sleep -Milliseconds 1200;$afterRead=Status telemetry
    if(-not $afterRead.paused -or $afterRead.ssdReadBytes -ne $paused.ssdReadBytes -or $afterRead.sourceReadBytes-$paused.sourceReadBytes -lt 16MB){throw 'Paused authoritative HDD reads were not confirmed.'}
    Operation resume;$result.finalRuntime=Status
    $result.fixtureCleanup=Client delete cleanup;$seeded=$false
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $logical=Get-Partition -DriveLetter D|Get-Disk
    $result.bootUnchanged=($before.machineSha256 -eq $after.machineSha256 -and $before.secureBootEnabled -eq $after.secureBootEnabled -and $before.codeIntegrityOptions -eq $after.codeIntegrityOptions)
    $result.sourceOriginalOffline=(Get-Disk -Number $source.Number).IsOffline
    $result.logicalDOnline=($logical.BusType -eq 'iSCSI' -and -not $logical.IsOffline -and $logical.Size -eq $source.Size)
    $result.cacheParentUnchanged=((Get-Partition -DriveLetter M|Get-Disk).UniqueId -eq $parent.UniqueId)
    $result.physicalDAccelerationActive=($result.finalRuntime.phase -eq 'active' -and $result.finalRuntime.canControl -and $result.logicalDOnline -and $result.sourceOriginalOffline)
    $result.passed=($result.physicalDAccelerationActive -and $result.bootUnchanged -and $result.cacheParentUnchanged -and -not(Test-Path -LiteralPath $clientInfo.fixture))
    if(-not $result.passed){throw 'Final native physical cache verification failed.'}
    Step 'active'
}catch{
    $result.error=$_.Exception.Message;$result.errorLocation=$_.ScriptStackTrace
    try{
        if($configured -and (Get-Service SuperHDDLoopbackService -ErrorAction SilentlyContinue)){
            Operation stop;$active=$false
            if((Get-Partition -DriveLetter D|Get-Disk).UniqueId -ne $source.UniqueId){throw 'Original source was not restored; owned state is preserved.'}
        }
        if($seeded){$result.fixtureCleanup=Client delete failure-cleanup;$seeded=$false}
        if($configured -and (Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json')){Operation remove}
    }catch{$result.cleanupError=$_.Exception.Message}
    $result.physicalDAccelerationActive=$active;$result.phase='failed';Save;throw
}finally{Save;Stop-Transcript|Out-Null}
