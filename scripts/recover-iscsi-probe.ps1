# Recovery for the exact disposable run recorded by test-iscsi-cache.ps1.
param([Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{32}$')][string]$RunId)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'remove-private-iscsi-portal.ps1')
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator required.'}
$record=Get-Content -LiteralPath (Join-Path $root 'artifacts\iscsi-local-latest.json') -Raw|ConvertFrom-Json
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$RunId)
$source='D:\SuperHDD-iSCSI-'+$RunId+'.img'
$cache=Join-Path $runRoot 'ssd-cache.bin'
$iqn='iqn.2026-10.local.superhdd:probe-'+$RunId
$exe=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
if($record.runId -ne $RunId -or $record.sourcePath -ne $source -or $record.cachePath -ne $cache -or (Get-Content -LiteralPath (Join-Path $runRoot 'ready') -Raw) -ne $iqn){throw 'Recorded disposable run identity mismatch.'}
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$RunId;passed=$false;error=$null}
try {
    $result.portalsBefore=@(Get-IscsiTargetPortal|Select-Object TargetPortalAddress,TargetPortalPortNumber,InitiatorPortalAddress,InitiatorInstanceName)
    $sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq $iqn)
    foreach($session in $sessions){
        $disks=@(Get-Disk -iSCSISession $session)
        if($disks.Count -ne 1 -or $disks[0].BusType -ne 'iSCSI' -or $disks[0].Size -ne 128MB -or $disks[0].SerialNumber.Trim() -ne $RunId -or $disks[0].IsBoot -or $disks[0].IsSystem){throw 'Disposable disk identity mismatch.'}
        $disks[0]|Set-Disk -IsOffline $true
        Disconnect-IscsiTarget -NodeAddress $iqn -Confirm:$false|Out-Null
    }
    Remove-PrivateIscsiPortal
    $processes=@(Get-CimInstance Win32_Process -Filter "Name='iscsi-probe.exe'"|Where-Object {$_.ExecutablePath -eq $exe -and $_.CommandLine -match ('server(?:-reopen)?(?:-512)?\s+'+$RunId+'(?:\s|$)')})
    if($processes.Count -gt 1){throw 'Unexpected duplicate disposable target.'}
    if($processes.Count -eq 1){
        $owned=Get-Process -Id $processes[0].ProcessId
        $token=[Guid]::NewGuid().ToString('N')
        Set-Content -LiteralPath (Join-Path $runRoot 'command.tmp') -Value ($token+' 5') -NoNewline
        Move-Item -LiteralPath (Join-Path $runRoot 'command.tmp') -Destination (Join-Path $runRoot 'command')
        if(-not $owned.WaitForExit(15000)){throw 'Disposable target did not stop.'}
        if((Get-Content -LiteralPath (Join-Path $runRoot 'ack') -Raw) -ne ($token+' 0')){throw 'Disposable target did not acknowledge shutdown.'}
    }
    if(Test-Path -LiteralPath $source){$result.sourceSha256=(Get-FileHash -LiteralPath $source).Hash}
    $recoveredCache=Join-Path $runRoot 'ssd-cache-recovered.bin'
    foreach($file in @($source,$cache,$recoveredCache)){if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}}
    if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Other initiator resources exist; preserve their service.'}
    if($record.initiatorBefore.status -eq 'Stopped'){Stop-Service MSiSCSI}
    Set-Service MSiSCSI -StartupType $record.initiatorBefore.startType
    $service=Get-Service MSiSCSI
    $result.initiatorRestored=([string]$service.Status -eq $record.initiatorBefore.status -and [string]$service.StartType -eq $record.initiatorBefore.startType)
    $result.sourceRemoved=-not(Test-Path -LiteralPath $source)
    $result.cacheRemoved=(-not(Test-Path -LiteralPath $cache) -and -not(Test-Path -LiteralPath $recoveredCache))
    $result.privateMountRemoved=-not(Test-Path -LiteralPath 'S:\fixture.bin')
    $result.passed=($result.initiatorRestored -and $result.sourceRemoved -and $result.cacheRemoved -and $result.privateMountRemoved)
}catch{$result.error=$_.Exception.Message;$result.errorDetail=($_|Format-List * -Force|Out-String);$result.errorStack=$_.ScriptStackTrace}finally{
    $result|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $runRoot 'cleanup-recovery.json') -Encoding utf8NoBOM
}
if(-not $result.passed){throw 'Disposable recovery failed; inspect cleanup-recovery.json.'}
