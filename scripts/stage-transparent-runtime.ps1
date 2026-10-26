# Workspace-only review package. Never installs, signs or loads a driver.
param([switch]$PreviewWithoutCurrentScmProof)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$kernel=Get-Content (Join-Path $root 'artifacts\transparent-session.json') -Raw|ConvertFrom-Json
$service=Get-Content (Join-Path $root 'artifacts\runtime\kernel-lab\transparent-service.json') -Raw|ConvertFrom-Json
if(-not $kernel.passed -or -not $kernel.probe.sourceRemainedMounted -or -not $service.passed -or -not $service.probe.sourcePageFileCoexistence -or -not $service.sourcePageFileRestored -or -not $service.finalHost.passed){throw 'Both complete owned-VM kernel and SCM proofs are required.'}
$vmDriver=Join-Path $root 'artifacts\driver-transparent-vm\SuperHDDCacheDriver.sys'
if((Get-FileHash $vmDriver).Hash -ne $kernel.unsignedDriverSha256 -or (Get-FileHash $vmDriver).Hash -ne $service.directDriver.unsignedSha256){throw 'VM proofs differ from the current kernel candidate.'}
$exe=Join-Path $root 'target\transparent-kernel\release\SuperHDDService.exe'
$proofHash=@($service.binaryHashes|Where-Object name -eq 'SuperHDDService.exe')[0].sha256
$serviceHash=(Get-FileHash $exe).Hash
if($serviceHash -ne $proofHash -and -not $PreviewWithoutCurrentScmProof){throw 'Current service differs from its complete SCM proof; rerun that proof before staging.'}
$out=Join-Path $root $(if($PreviewWithoutCurrentScmProof){'artifacts\transparent-runtime-preview'}else{'artifacts\transparent-runtime-candidate'})
if(Test-Path $out){throw 'An existing staged candidate is preserved.'}
New-Item -ItemType Directory -Path $out|Out-Null
$files=[ordered]@{
    'SuperHDDCacheV7.sys'=(Join-Path $root 'artifacts\driver-transparent\SuperHDDCacheDriver.sys')
    'SuperHDDService.exe'=$exe
    'SuperHDD.exe'=(Join-Path $root 'target\transparent-kernel\release\SuperHDD.exe')
}
$hashes=[ordered]@{}
foreach($name in $files.Keys){Copy-Item -LiteralPath $files[$name] -Destination (Join-Path $out $name);$hashes[$name]=(Get-FileHash (Join-Path $out $name)).Hash}
$sources=@('drivers\superhdd-cache-driver\v7\volume_driver.c','drivers\superhdd-cache-driver\v7\protocol_v7.h','drivers\superhdd-cache-driver\v7\source_policy.h','drivers\superhdd-cache-driver\v7\usage_state.h','drivers\superhdd-cache-driver\v7\direct_source.inc','drivers\superhdd-cache-driver\v7\raw_backend.inc','crates\platform\src\transparent_driver.rs','crates\platform\src\transparent_cache.rs','crates\platform\src\transparent_runtime.rs','services\superhdd-service\src\transparent_host.rs','services\superhdd-service\src\transparent_ops.rs','installer\enroll-transparent-source.ps1')
$sourceHashes=@($sources|ForEach-Object {Get-FileHash (Join-Path $root $_)|Select-Object Path,Hash})
$signature=Get-AuthenticodeSignature (Join-Path $out 'SuperHDDCacheV7.sys')
[ordered]@{at=[DateTime]::UtcNow.ToString('o');protocolVersion=7;backend='storageFilter';reviewPreview=[bool]$PreviewWithoutCurrentScmProof;files=$hashes;sourceHashes=$sourceHashes;kernelVmRun=$kernel.runId;scmVmRun=$service.runId;scmTestedServiceSha256=$proofHash;stagedServiceMatchesScmProof=($serviceHash -eq $proofHash);currentScmRetestPending=($serviceHash -ne $proofHash);pagingReads='passthrough';dedicatedRamDataCache=$false;pageFileRelocationRequired=$false;signatureStatus=$signature.Status.ToString();productionQualified=$false;hostDriverInstalled=$false;physicalDAccelerationActive=$false;remainingHostGate='Windows-accepted kernel signature and physical-machine qualification'}|ConvertTo-Json -Depth 7|Set-Content (Join-Path $out 'manifest.json') -Encoding utf8NoBOM
