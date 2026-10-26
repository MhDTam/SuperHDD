# Workspace-only review artifact. Does not sign, install, load, or enable a driver.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$proofPath=Join-Path $root 'artifacts\runtime\kernel-lab\paging-service.json'
$proof=Get-Content $proofPath -Raw|ConvertFrom-Json
if($proof.phase -ne 'completed' -or -not $proof.passed -or -not $proof.probe.passed -or -not $proof.applicationKernelProbe.passed -or -not $proof.applicationKernelProbe.applicationWorkload.passed -or -not $proof.sourcePageFileRestored -or -not $proof.hostParentsRemoved -or -not $proof.coldBootCleanup.passed -or -not $proof.finalHost.passed -or $proof.finalHost.vmState -ne 'Off'){throw 'Complete SCM, paging workload and cleanup evidence is required.'}
$vm=Join-Path $root 'artifacts\driver-paging-vm\SuperHDDCacheDriver.sys'
if((Get-FileHash $vm).Hash -ne $proof.directDriver.unsignedSha256){throw 'VM driver differs from completed proof.'}
$service=Join-Path $root 'target\paging-kernel\release\SuperHDDService.exe'
$serviceProof=@($proof.binaryHashes|Where-Object name -eq 'SuperHDDService.exe')[0].sha256
if((Get-FileHash $service).Hash -ne $serviceProof){throw 'Service differs from completed proof.'}
$uiPath=Join-Path $root 'artifacts\paging-ui-tests.json'
$ui=Get-Content $uiPath -Raw|ConvertFrom-Json
if($ui.stats.expected -lt 23 -or $ui.stats.unexpected -ne 0 -or $ui.stats.skipped -ne 0){throw 'Current complete UI evidence is required.'}
$out=Join-Path $root 'artifacts\paging-runtime-candidate'
if(Test-Path $out){throw 'Preserve existing candidate.'}
New-Item -ItemType Directory $out|Out-Null
$files=[ordered]@{
    'SuperHDDCacheV8.sys'=(Join-Path $root 'artifacts\driver-paging\SuperHDDCacheDriver.sys')
    'SuperHDDService.exe'=$service
    'SuperHDD.exe'=(Join-Path $root 'artifacts\paging-desktop\SuperHDD.exe')
}
$hashes=[ordered]@{}
foreach($name in $files.Keys){Copy-Item -LiteralPath $files[$name] -Destination (Join-Path $out $name);$hashes[$name]=(Get-FileHash (Join-Path $out $name)).Hash}
$sources=@('crates\platform\src\paging_driver.rs','crates\platform\src\paging_cache.rs','crates\platform\src\paging_runtime.rs','crates\platform\src\cache_runtime.rs','crates\platform\src\lib.rs','crates\platform\src\driver.rs','services\superhdd-service\src\paging_host.rs','services\superhdd-service\src\paging_ops.rs','services\superhdd-service\src\main.rs','installer\enroll-paging-source.ps1','apps\desktop\src\App.tsx','crates\platform\examples\paging_session_probe.rs','crates\platform\examples\paging_service_probe.rs','crates\platform\examples\support\paging_workload.rs')
$sourceHashes=@($sources|ForEach-Object{Get-FileHash (Join-Path $root $_)|Select-Object Path,Hash})
$sourceHashes+=@(Get-ChildItem (Join-Path $root 'drivers\superhdd-cache-driver\v8') -File|ForEach-Object{Get-FileHash $_.FullName|Select-Object Path,Hash})
$sourceHashes+=@(Get-ChildItem (Join-Path $root 'drivers\superhdd-cache-driver') -File|Where-Object Extension -in '.h','.inc'|ForEach-Object{Get-FileHash $_.FullName|Select-Object Path,Hash})
$signature=Get-AuthenticodeSignature (Join-Path $out 'SuperHDDCacheV8.sys')
$work=$proof.applicationKernelProbe.applicationWorkload
[ordered]@{at=[DateTime]::UtcNow.ToString('o');protocolVersion=8;backend='storageFilter';files=$hashes;sourceHashes=$sourceHashes;scmVmRun=$proof.runId;proofSha256=(Get-FileHash $proofPath).Hash;uiProofSha256=(Get-FileHash $uiPath).Hash;stagedServiceMatchesScmProof=$true;currentScmRetestPending=$false;bufferedPagingSsdObserved=$work.bufferedPagingSsdObserved;mappedPagingSsdObserved=$work.mappedPagingSsdObserved;allTimingPairsObserveStorage=$work.allTimingPairsObserveStorage;dedicatedRamDataCache=$false;reservedReadScratchBytes=65536;pageFileRelocationRequired=$false;signatureStatus=$signature.Status.ToString();productionQualified=$false;hostDriverInstalled=$false;physicalDAccelerationActive=$false;remainingHostGate='Windows-accepted kernel signature and physical-machine paging/power/PnP qualification'}|ConvertTo-Json -Depth 7|Set-Content (Join-Path $out 'manifest.json') -Encoding utf8NoBOM
