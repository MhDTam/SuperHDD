param([switch]$Recovery,[switch]$Handoff,[switch]$NativeInitiator,[ValidateSet(512,4096)][int]$SectorBytes=4096)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$log=Join-Path $root 'artifacts\iscsi-local-run.log'
Start-Transcript -LiteralPath $log -Force | Out-Null
try {
    & (Join-Path $PSScriptRoot 'test-iscsi-cache.ps1') -Recovery:$Recovery -Handoff:$Handoff -NativeInitiator:$NativeInitiator -SectorBytes $SectorBytes
}catch{
    $_ | Format-List * -Force | Out-String | Write-Output
    exit 1
}finally{Stop-Transcript | Out-Null}
