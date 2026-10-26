# User-mode loopback proof on owned files only. No administrator or OS disk needed.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$run)
$record=Join-Path $root 'artifacts\iscsi-protocol-latest.json'
$exe=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
$source='D:\SuperHDD-iSCSI-'+$run+'.img'
$cache=Join-Path $runRoot 'ssd-cache.bin'
$built=Get-Content -LiteralPath (Join-Path (Split-Path $exe) 'build.json') -Raw|ConvertFrom-Json
foreach($item in @($built.sources)+@($built.binaries)){if((Get-FileHash -LiteralPath $item.Path).Hash -ne $item.Hash){throw 'Rebuild the probe after changing source or binaries.'}}
if(Get-NetTCPConnection -State Listen -LocalPort 33261 -ErrorAction SilentlyContinue){throw 'Private protocol port is in use.'}
$hdd=Get-Partition -DriveLetter D|Get-Disk
$ssd=Get-Partition -DriveLetter M|Get-Disk
if($hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $ssd.FriendlyName -notmatch '980 PRO' -or $hdd.UniqueId -eq $ssd.UniqueId){throw 'Expected D HDD and M SSD unavailable.'}
if((Get-Volume D).SizeRemaining -lt 1GB -or (Get-Volume M).SizeRemaining -lt 1GB){throw 'Insufficient space for disposable files.'}
New-Item -ItemType Directory -Path $runRoot|Out-Null
$before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$service=Get-Service MSiSCSI
$lock=[IO.File]::Open((Join-Path $root 'artifacts\runtime\kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;passed=$false;scope='User-mode loopback protocol with D fixture and M cache; not Windows initiator or existing D interception';error=$null;bootBefore=$before;newKernelDriverInstalled=$false;existingVolumesRemapped=$false}
$result.binaries=@('iscsi-probe.exe','superhdd_iscsi_probe.dll','ISCSI.dll','DiskAccessLibrary.dll'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path (Split-Path $exe) $_)|Select-Object Path,Hash})
$result.sourceHashes=@('crates\cache-core\src\lib.rs','tools\iscsi-cache-probe\src\lib.rs','tools\iscsi-cache-probe\Probe.cs','tools\iscsi-cache-probe\Protocol.cs','scripts\test-iscsi-protocol.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
try {
    $result.build=$built
    & $exe protocol $run *> (Join-Path $runRoot 'protocol.log')
    if($LASTEXITCODE){throw 'Loopback protocol test failed.'}
    $result.probe=Get-Content -LiteralPath (Join-Path $runRoot 'protocol.json') -Raw|ConvertFrom-Json
    $inside=Get-Content -LiteralPath (Join-Path $runRoot 'watch-inside.json') -Raw|ConvertFrom-Json
    $outside=Get-Content -LiteralPath (Join-Path $runRoot 'watch-outside.json') -Raw|ConvertFrom-Json
    if($inside.watchedStatistics.sourceReadBytes -lt 8192 -or $outside.statistics.sourceReadBytes -le $inside.statistics.sourceReadBytes -or $outside.watchedStatistics.sourceReadBytes -ne $inside.watchedStatistics.sourceReadBytes -or $outside.watchedStatistics.ssdReadBytes -ne $inside.watchedStatistics.ssdReadBytes){throw 'Fixture traffic observer did not exclude unrelated reads.'}
    $result.fixtureObserverVerified=$true
    $cold=Get-Content -LiteralPath (Join-Path $runRoot 'cold.json') -Raw|ConvertFrom-Json
    $warm=Get-Content -LiteralPath (Join-Path $runRoot 'warm.json') -Raw|ConvertFrom-Json
    $pause=Get-Content -LiteralPath (Join-Path $runRoot 'paused.json') -Raw|ConvertFrom-Json
    $mutation=Get-Content -LiteralPath (Join-Path $runRoot 'mutated.json') -Raw|ConvertFrom-Json
    $result.warmDelta=[ordered]@{hits=$warm.statistics.hits-$cold.statistics.hits;ssdReadBytes=$warm.statistics.ssdReadBytes-$cold.statistics.ssdReadBytes;sourceReadBytes=$warm.statistics.sourceReadBytes-$cold.statistics.sourceReadBytes}
    if($result.warmDelta.hits -lt 96 -or $result.warmDelta.ssdReadBytes -lt 6MB -or $result.warmDelta.sourceReadBytes -ne 0){throw 'Warm SSD traffic check failed.'}
    if($pause.statistics.sourceReadBytes - $warm.statistics.sourceReadBytes -lt 6MB -or $pause.statistics.ssdReadBytes -ne $warm.statistics.ssdReadBytes){throw 'Pause bypass check failed.'}
    # Two setup writes verify 512-byte RMW, followed by 256 fixture writes and a patch.
    if($mutation.statistics.invalidations -le 0 -or $mutation.writes -ne 259){throw 'Write-through invalidation check failed.'}
    $result.passed=$result.probe.passed
}catch{$result.error=$_.Exception.Message}finally{
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootAfter=$after
    $result.bootUnchanged=($before.machineSha256 -eq $after.machineSha256 -and $before.secureBootEnabled -eq $after.secureBootEnabled -and $before.codeIntegrityOptions -eq $after.codeIntegrityOptions)
    $now=Get-Service MSiSCSI
    $result.initiatorUnchanged=($now.Status -eq $service.Status -and $now.StartType -eq $service.StartType)
    $result.ownedFilesRemoved=(-not(Test-Path -LiteralPath $source) -and -not(Test-Path -LiteralPath $cache))
    $result.portClosed=-not(Get-NetTCPConnection -State Listen -LocalPort 33261 -ErrorAction SilentlyContinue)
    $result.passed=[bool]($result.passed -and $result.bootUnchanged -and $result.initiatorUnchanged -and $result.ownedFilesRemoved -and $result.portClosed)
    $result|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $record -Encoding utf8NoBOM
    $lock.Dispose()
}
[pscustomobject]@{passed=$result.passed;runId=$run;error=$result.error;warmDelta=$result.warmDelta;bootUnchanged=$result.bootUnchanged;ownedFilesRemoved=$result.ownedFilesRemoved;record=$record}|ConvertTo-Json -Depth 5
if(-not $result.passed){throw 'User-mode protocol proof did not pass.'}
