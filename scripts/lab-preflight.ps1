param([string]$LabDriver,[switch]$Pnp)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
if(-not $LabDriver){$LabDriver=Join-Path $root $(if($Pnp){'artifacts\driver-pnp-lab\SuperHDDCacheDriver.sys'}else{'artifacts\driver-lab\SuperHDDCacheDriver.sys'})}
$elevated=([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$signature=Get-AuthenticodeSignature -FilePath $LabDriver
$vmAuthorized=$false;$vmReason=$null;$vmCount=0
try {$machines=@(Get-VM -ErrorAction Stop);$vmCount=$machines.Count;$vmAuthorized=$true}catch{$vmReason=$_.Exception.Message}
$blockers=@('Kernel runtime, mounted-volume/PnP integration and reliability matrix are not qualified.')
if($Pnp){$blockers+='PnP lab source is implemented but per-volume deployment and ordinary-file probe have not run.';$blockers+='Paging cache hits, persistence, recursive-device ancestry and physical-HDD I/O proof are incomplete.'}
if(-not $elevated){$blockers+='Current Windows process is not elevated.'}
if($signature.Status -ne 'Valid'){$blockers+='Experimental driver is unsigned or its signature is not trusted.'}
if(-not $vmAuthorized){$blockers+='Host policy denies Hyper-V VM access.'}
if($vmCount -eq 0){$blockers+='No authorized isolated lab VM was selected or demonstrated.'}
$report=[pscustomobject]@{
    at=[DateTime]::UtcNow.ToString('o');readyToRunKernelTests=$false;readOnly=$true
    elevated=$elevated;signatureStatus=$signature.Status.ToString();driverSha256=(Get-FileHash -LiteralPath $LabDriver).Hash
    hypervAuthorized=$vmAuthorized;accessibleVmCount=$vmCount;hypervReason=$vmReason;blockers=$blockers
    installedDriver=$false;changedSecuritySettings=$false;allocatedCache=$false
    protocolVersion=$(if($Pnp){3}else{2});variant=$(if($Pnp){'mounted-volume-pnp-lab'}else{'raw-vhd-lab'})
}
$report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $root $(if($Pnp){'artifacts\runtime\lab-preflight-v3.json'}else{'artifacts\runtime\lab-preflight-v2.json'})) -Encoding utf8NoBOM
$report | ConvertTo-Json -Depth 5
