$ErrorActionPreference = 'Stop'
$WarningPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$physical = @{}
try { Get-PhysicalDisk | ForEach-Object { $physical[[string]$_.DeviceId] = [string]$_.MediaType } } catch {}
$encryptionByMountPoint = @{}
try { Get-BitLockerVolume -ErrorAction Stop | ForEach-Object { $encryptionByMountPoint[[string]$_.MountPoint] = [string]$_.VolumeStatus } } catch {}
$disks = @(Get-Disk | ForEach-Object {
    $disk = $_
    $kind = $physical[[string]$disk.Number]
    if ($kind -notin @('SSD','HDD')) { $kind = 'Unknown' }
    $volumes = @(Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter -and [int][char]$_.DriveLetter -ne 0 } | ForEach-Object {
        $partition = $_
        $volume = $partition | Get-Volume -ErrorAction SilentlyContinue
        if ($volume) {
            $encryption = 'Unknown'
            $knownEncryption = $encryptionByMountPoint[([string]$partition.DriveLetter) + ':']
            if ($knownEncryption) { $encryption = $knownEncryption }
            [pscustomobject]@{letter=([string]$partition.DriveLetter)+':';label=[string]$volume.FileSystemLabel;fileSystem=[string]$volume.FileSystemType;size=[long]$volume.Size;free=[long]$volume.SizeRemaining;partitionNumber=[int]$partition.PartitionNumber;system=[bool]($partition.IsBoot -or $partition.IsSystem);health=[string]$volume.HealthStatus;encryption=$encryption}
        }
    })
    [pscustomobject]@{number=[int]$disk.Number;id=[string]$disk.UniqueId;model=[string]$disk.FriendlyName;serial=[string]$disk.SerialNumber;kind=$kind;bus=[string]$disk.BusType;size=[long]$disk.Size;online=[bool](-not $disk.IsOffline);readOnly=[bool]$disk.IsReadOnly;system=[bool]($disk.IsBoot -or $disk.IsSystem);volumes=$volumes}
})
$drivers = @(Get-CimInstance Win32_SystemDriver | Where-Object {$_.Name -in @('SuperHDDCacheDriver','SuperHDDRawLab')} | Select-Object @{n='name';e={$_.Name}},@{n='state';e={$_.State}})
$service = Get-Service -Name SuperHDDService -ErrorAction SilentlyContinue
[pscustomobject]@{disks=$disks;drivers=$drivers;serviceState=$(if($service){[string]$service.Status}else{'NotInstalled'});collectedAt=(Get-Date).ToUniversalTime().ToString('o')} | ConvertTo-Json -Depth 8 -Compress
