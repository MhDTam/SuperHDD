param([switch]$ResumeManaged)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');passed=$false;phase='container';error=$null}
$output=Join-Path $workspace 'artifacts\runtime\cache-control-verification.json'
function Save-Result{$result|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $output -Encoding utf8NoBOM}
try{
    if(Test-Path -LiteralPath $output){Move-Item -LiteralPath $output -Destination (Join-Path $workspace ('artifacts\runtime\cache-control-verification-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff')+'.json'))}
    Save-Result
    if($ResumeManaged){
        # Reuse only already passing exact-source/exact-image component proofs.
        # This avoids rebooting the VM for an unchanged read-only bridge after
        # editing only the managed-session probe.
        $proofFiles=@('cache-session-host-refusal.json','native-bridge-host-guard.json','cache-container-result.json','cache-lifecycle-result.json','kernel-lab\native-bridge.json')
        $result.reusedProofs=@()
        foreach($file in $proofFiles){
            $proof=Get-Content -LiteralPath (Join-Path $workspace ('artifacts\runtime\'+$file)) -Raw|ConvertFrom-Json
            if(-not $proof.passed){throw ('Cannot reuse a failing proof: '+$file)}
            foreach($item in $proof.sourceHashes){if((Get-FileHash -LiteralPath $item.Path).Hash -ne $item.Hash){throw ('Reused proof source changed: '+$item.Path)}}
            $result.reusedProofs+=@{file=$file;at=$proof.at}
        }
        $serviceHash=(Get-FileHash -LiteralPath (Join-Path $workspace 'target\release\SuperHDDService.exe')).Hash
        $guard=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\runtime\cache-session-host-refusal.json') -Raw|ConvertFrom-Json
        $bridge=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\runtime\kernel-lab\native-bridge.json') -Raw|ConvertFrom-Json
        if($guard.serviceSha256 -ne $serviceHash -or -not($bridge.binaryHashes|Where-Object {$_.name -eq 'SuperHDDService.exe' -and $_.sha256 -eq $serviceHash})){throw 'Reused service image changed.'}
        foreach($entry in @(@{proof='cache-container-result.json';tool='container_probe.exe'},@{proof='cache-lifecycle-result.json';tool='container_lifecycle_probe.exe'})){
            $proof=Get-Content -LiteralPath (Join-Path $workspace ('artifacts\runtime\'+$entry.proof)) -Raw|ConvertFrom-Json
            if($proof.toolSha256 -ne (Get-FileHash -LiteralPath (Join-Path $workspace ('target\release\examples\'+$entry.tool))).Hash){throw 'Reused container probe image changed.'}
        }
        $bridgeHash=(Get-FileHash -LiteralPath (Join-Path $workspace 'target\release\examples\native_bridge_probe.exe')).Hash
        if(-not($bridge.binaryHashes|Where-Object {$_.name -eq 'native_bridge_probe.exe' -and $_.sha256 -eq $bridgeHash})){throw 'Reused bridge probe image changed.'}
    }else{
    & (Join-Path $PSScriptRoot 'test-cache-session-host-guard.ps1')
    & (Join-Path $PSScriptRoot 'test-native-bridge-host-guard.ps1')
    & (Join-Path $PSScriptRoot 'test-cache-container.ps1')
    $result.phase='lifecycle';Save-Result
    & (Join-Path $PSScriptRoot 'test-cache-lifecycle.ps1')
    $result.phase='service-bridge';Save-Result
    & (Join-Path $PSScriptRoot 'test-native-bridge.ps1')
    }
    $result.phase='managed-session';Save-Result
    & (Join-Path $PSScriptRoot 'test-managed-session.ps1')
    $result.phase='scm-recovery';Save-Result
    & (Join-Path $PSScriptRoot 'test-managed-session.ps1') -ScmRecovery
    $result.phase='completed';$result.passed=$true
}catch{$result.phase='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}finally{Save-Result}
if(-not $result.passed){throw $result.error}
