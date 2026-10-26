# No install/upgrade, global driver stop, firmware, BCD, or existing-volume remap.
# Only a private read-only instance backed by a new 16 MiB D: fixture.
param([switch]$DirectoryMount)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $root 'artifacts\runtime'
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $runtime ('dokan-local-'+$run)
$record=Join-Path $root ('artifacts\dokan-local-'+$run+'.json')
$tool=Join-Path $root 'target\release\examples\dokan_cache_probe.exe'
$bridge=Join-Path $root 'artifacts\dokan-probe\bridge.dll'
$driver='C:\Windows\System32\drivers\dokan2.sys'
$dll='C:\Windows\System32\dokan2.dll'
function Parents {
    $hdd=Get-Partition -DriveLetter D|Get-Disk
    $ssd=Get-Partition -DriveLetter M|Get-Disk
    $dv=Get-Volume -DriveLetter D
    $mv=Get-Volume -DriveLetter M
    if($hdd.IsBoot -or $hdd.IsSystem -or $hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $ssd.FriendlyName -notmatch '980 PRO' -or $hdd.UniqueId -eq $ssd.UniqueId -or $dv.SizeRemaining -lt 1GB -or $mv.SizeRemaining -lt 1GB -or $dv.FileSystem -ne 'NTFS' -or $mv.FileSystem -ne 'NTFS'){throw 'Expected separate D HDD and M SSD are unavailable.'}
    [pscustomobject]@{sourceDisk=[string]$hdd.UniqueId;sourceVolume=[string]$dv.UniqueId;cacheDisk=[string]$ssd.UniqueId;cacheVolume=[string]$mv.UniqueId}
}
$before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$parents=Parents
foreach($path in @($tool,$bridge,$driver,$dll)){if(-not(Test-Path -LiteralPath $path)){throw 'Build the optional probe and use the existing Dokan 2.3.1 runtime first.'}}
$signedDriver=Get-AuthenticodeSignature -LiteralPath $driver
$signedDll=Get-AuthenticodeSignature -LiteralPath $dll
$driverState=Get-CimInstance Win32_SystemDriver -Filter "Name='Dokan2'"
if($driverState.State -ne 'Running' -or $signedDriver.Status -ne 'Valid' -or $signedDriver.SignerCertificate.Subject -notlike 'CN=Microsoft Windows Hardware Compatibility Publisher,*' -or $signedDll.Status -ne 'Valid' -or (Get-Item -LiteralPath $dll).VersionInfo.FileVersion -ne '2.3.1.1000'){throw 'Existing signed Dokan runtime verification failed; no install or upgrade will be attempted.'}
if(-not $DirectoryMount -and (Get-PSDrive -Name S -ErrorAction SilentlyContinue)){throw 'S is already in use; existing mounts are preserved.'}
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;passed=$false;scope='NEW read-only native host filesystem proof only; existing data volumes are not intercepted';bootBefore=$before;parents=$parents;driverName=$driverState.Name;driverSignature=$signedDriver.Status.ToString();driverSigner=$signedDriver.SignerCertificate.Subject;driverSha256=(Get-FileHash -LiteralPath $driver).Hash;dllSha256=(Get-FileHash -LiteralPath $dll).Hash;probeSha256=(Get-FileHash -LiteralPath $tool).Hash;bridgeSha256=(Get-FileHash -LiteralPath $bridge).Hash;newKernelDriverInstalled=$false;existingVolumeLettersChanged=$false;error=$null}
$result.sourceHashes=@('crates\cache-core\src\lib.rs','crates\cache-core\examples\dokan_cache_probe.rs','tools\dokan-probe\bridge.cpp','scripts\build-dokan-probe.ps1','scripts\test-dokan-cache.ps1','scripts\local-boot-state.ps1'|ForEach-Object {Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sdkVersion='v2.3.1.1000'
$result.directoryMount=[bool]$DirectoryMount
$directoryParent='D:\SuperHDD-DokanDirectory-'+$run
$directoryPoint=Join-Path $directoryParent 'visible'
$result.sdkHeaderHashes=@('dokan.h','fileinfo.h','public.h'|ForEach-Object {Get-FileHash -LiteralPath (Join-Path $root ('.tools\dokan-sdk\2.3.1\'+$_))|Select-Object Path,Hash})
$lock=[IO.File]::Open((Join-Path $runtime 'kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try {
    New-Item -ItemType Directory -Path $runRoot|Out-Null
    $result|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $record -Encoding utf8NoBOM
    $probeArguments=@($runRoot,$bridge)
    if($DirectoryMount){$probeArguments+='--directory-mount'}
    & $tool @probeArguments *> (Join-Path $root ('artifacts\dokan-local-'+$run+'.log'))
    $result.probeExitCode=$LASTEXITCODE
    if($result.probeExitCode){throw ('Native Dokan/cache check failed (exit '+$result.probeExitCode+'). Inspect the run log; only the new test files may remain.')}
    $result.probe=Get-Content -LiteralPath (Join-Path $runRoot 'result.json') -Raw|ConvertFrom-Json
    if(-not $result.probe.passed -or -not $result.probe.ownedFilesRemoved){throw 'Native checks or owned-file removal failed.'}
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootAfter=$after
    $now=Parents
    $result.parentsUnchanged=($parents.sourceDisk -eq $now.sourceDisk -and $parents.sourceVolume -eq $now.sourceVolume -and $parents.cacheDisk -eq $now.cacheDisk -and $parents.cacheVolume -eq $now.cacheVolume)
    $result.bootUnchanged=($before.machineSha256 -eq $after.machineSha256 -and $before.secureBootEnabled -eq $after.secureBootEnabled -and $before.codeIntegrityOptions -eq $after.codeIntegrityOptions)
    $result.runtimeUnchanged=((Get-FileHash -LiteralPath $driver).Hash -eq $result.driverSha256 -and (Get-FileHash -LiteralPath $dll).Hash -eq $result.dllSha256 -and (Get-CimInstance Win32_SystemDriver -Filter "Name='Dokan2'").State -eq 'Running')
    $result.sourceRemoved=if($DirectoryMount){-not(Test-Path -LiteralPath $directoryParent)}else{-not(Test-Path -LiteralPath ('D:\SuperHDD-Dokan-'+$run+'.bin'))}
    $result.cacheRemoved=(-not(Test-Path -LiteralPath (Join-Path $runRoot 'ssd-cache.bin')))
    $result.ownedMountRemoved=if($DirectoryMount){-not(Test-Path -LiteralPath $directoryPoint)}else{-not(Test-Path -LiteralPath 'S:\fixture.bin')}
    $result.passed=[bool]($result.passed -and $result.parentsUnchanged -and $result.bootUnchanged -and $result.runtimeUnchanged -and $result.sourceRemoved -and $result.cacheRemoved -and $result.ownedMountRemoved)
    $result|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $record -Encoding utf8NoBOM
    $lock.Dispose()
}
[pscustomobject]@{passed=$result.passed;runId=$run;record=$record;error=$result.error;bootUnchanged=$result.bootUnchanged;ownedFilesRemoved=($result.sourceRemoved -and $result.cacheRemoved);ownedMountRemoved=$result.ownedMountRemoved}|ConvertTo-Json
if(-not $result.passed){throw 'Existing-runtime host proof did not pass.'}
