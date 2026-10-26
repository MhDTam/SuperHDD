param([switch]$Start,[switch]$ResumeOwnedImage,[switch]$SkipApplyOwnedImage)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$report=Join-Path $lab 'provisioning.json'
$name='SuperHDD-KernelLab-20261019'
$diskPath=Join-Path $lab 'os.vhdx'
$iso=Join-Path $lab 'windows-server-2022-eval.iso'
$state=[ordered]@{startedAt=[DateTime]::UtcNow.ToString('o');vmName=$name;phase='checking';hostSecurityChanged=$false;networkAdapters=0;error=$null}
function Save-State { $state | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $report -Encoding utf8NoBOM }
Save-State
$mountedVhd=$false;$mountedIso=$false
try {
    $principal=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Run through normal administrator elevation.'}
    if(Get-VM -Name $name -ErrorAction SilentlyContinue){throw 'The lab VM already exists; refusing replacement.'}
    if((Test-Path -LiteralPath $diskPath) -and -not $ResumeOwnedImage){throw 'The lab OS disk already exists; refusing replacement.'}
    if($ResumeOwnedImage -and -not (Test-Path -LiteralPath $diskPath)){throw 'No owned image exists to resume.'}
    if(Get-Volume -DriveLetter Z -ErrorAction SilentlyContinue){throw 'Temporary drive Z is already in use.'}
    if((Get-Item -LiteralPath $iso).Length -lt 4GB){throw 'Evaluation ISO is incomplete.'}
    $state.phase='applying-official-evaluation-image';Save-State
    $mounted=Mount-DiskImage -ImagePath $iso -PassThru -Access ReadOnly
    $mountedIso=$true
    $isoVolume=$mounted | Get-Volume
    $isoImage=Join-Path ($isoVolume.DriveLetter+':\') 'sources\install.wim'
    $image=Join-Path $lab 'install.wim'
    if(-not (Test-Path -LiteralPath $image)){Copy-Item -LiteralPath $isoImage -Destination $image}
    $imageInfo=Get-WindowsImage -ImagePath $image -Index 1
    if($imageInfo.ImageName -notmatch 'Windows Server 2022.*Evaluation'){throw 'Unexpected OS image.'}
    $state.imageName=$imageInfo.ImageName;Save-State
    if(-not $ResumeOwnedImage){New-VHD -Path $diskPath -SizeBytes 32GB -Dynamic | Out-Null}
    $vhd=Mount-VHD -Path $diskPath -PassThru
    $mountedVhd=$true
    $disk=$vhd | Get-Disk
    $identity=Get-VHD -DiskNumber $disk.Number
    if($disk.IsBoot -or $disk.IsSystem -or ($disk.PartitionStyle -ne 'RAW' -and -not $ResumeOwnedImage) -or
        $identity.Path -ne $diskPath -or $disk.BusType -ne 'File Backed Virtual'){
        throw 'New disposable VHD identity checks failed; no disk changes permitted.'
    }
    if($ResumeOwnedImage){
        $partitions=@(Get-Partition -DiskNumber $disk.Number)
        if($identity.Size -ne 32GB -or $partitions.Count -ne 1 -or ($partitions[0]|Get-Volume).FileSystemLabel -ne 'SHDD-Lab-OS' -or $partitions[0].DriveLetter -ne 'Z'){
            throw 'Owned partial OS image identity check failed.'
        }
    }else{
        Initialize-Disk -Number $disk.Number -PartitionStyle MBR | Out-Null
        $partition=New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter Z -IsActive
        $partition | Format-Volume -FileSystem NTFS -NewFileSystemLabel 'SHDD-Lab-OS' -Confirm:$false | Out-Null
    }
    if($SkipApplyOwnedImage -and -not $ResumeOwnedImage){throw 'Skip apply is permitted only for the verified owned partial image.'}
    if(-not $SkipApplyOwnedImage){
        & dism.exe /Apply-Image ('/ImageFile:'+$image) /Index:1 /ApplyDir:Z:\ /CheckIntegrity *> (Join-Path $lab 'apply-image.log')
        if($LASTEXITCODE){throw "DISM image application failed: $LASTEXITCODE"}
    }
    if(-not(Test-Path -LiteralPath Z:\Windows\System32\ntoskrnl.exe)){throw 'The owned OS image is incomplete.'}
    Dismount-VHD -Path $diskPath;$mountedVhd=$false
    Dismount-DiskImage -ImagePath $iso | Out-Null;$mountedIso=$false
    $state.phase='os-image-applied'
    $state.finishedAt=[DateTime]::UtcNow.ToString('o');Save-State
    # Initialize this guest's BIOS boot files from its own isolated WinPE.
    # Never open or change the host BCD store.
    if($Start){& (Join-Path $PSScriptRoot 'start-kernel-lab-recovery.ps1')}
} catch {
    $state.error=$_.Exception.Message;$state.phase='failed';Save-State;throw
} finally {
    if($mountedVhd){Dismount-VHD -Path $diskPath -ErrorAction Continue}
    if($mountedIso){Dismount-DiskImage -ImagePath $iso -ErrorAction Continue | Out-Null}
}
