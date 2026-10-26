$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
$vm=Get-VM -Id $configuration.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019'){throw 'VM identity mismatch.'}
$credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
$session=New-PSSession -VMId $vm.Id -Credential $credential
try{
    $r=Invoke-Command -Session $session -ScriptBlock {
        [ordered]@{at=[DateTime]::UtcNow.ToString('o');lastBoot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime;disks=@(Get-Disk|Select-Object Number,UniqueId,Size,BusType,IsOffline);volumes=@(Get-Volume|Select-Object DriveLetter,FileSystemLabel,FileSystem,HealthStatus,Size);driver=(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'"|Select-Object State,PathName);reattachLog=(Get-Content C:\SuperHDDLab\reattach-volumes.log -Raw -ErrorAction SilentlyContinue);serviceLog=(Get-Content C:\SuperHDDLab\service-stop.log -Raw -ErrorAction SilentlyContinue);systemEvents=@(Get-WinEvent -FilterHashtable @{LogName='System';StartTime=(Get-Date).AddMinutes(-40);Level=1,2,3} -MaxEvents 15 -ErrorAction SilentlyContinue|Select-Object TimeCreated,Id,ProviderName,Message)}
    }
    $r.hostAttachments=@($vm|Get-VMHardDiskDrive|Select-Object ControllerType,Path)
    $r.driverDiagnostic=Invoke-Command -Session $session -ScriptBlock {& C:\SuperHDDLab\raw_lab.exe diagnose|ConvertFrom-Json}
    $r|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $lab 'inspection.json') -Encoding utf8NoBOM
}finally{Remove-PSSession $session}
