$ErrorActionPreference='Stop'
$result=[ordered]@{boot=(& (Join-Path $PSScriptRoot 'local-boot-state.ps1'));encryption=@();encryptionReadError=$null}
try {
    $result.encryption=@(Get-BitLockerVolume -MountPoint 'C:','E:' | Select-Object MountPoint,@{n='volumeStatus';e={$_.VolumeStatus.ToString()}},@{n='protectionStatus';e={$_.ProtectionStatus.ToString()}},EncryptionPercentage)
} catch {$result.encryptionReadError=$_.Exception.Message}
# Read-only inspection: never collect recovery passwords or modify protectors.
$result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot '..\artifacts\local-boot-inspection.json') -Encoding utf8NoBOM
