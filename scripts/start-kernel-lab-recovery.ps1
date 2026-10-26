# Boot repair of the newly created disposable OS VHD, inside WinPE.
# No host BCD store or physical partition is edited by this routine.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$name='SuperHDD-KernelLab-20261019'
$diskPath=Join-Path $lab 'os.vhdx'
$state=[ordered]@{startedAt=[DateTime]::UtcNow.ToString('o');vmName=$name;phase='repair-bootstrap';hostSecurityChanged=$false;networkAdapters=0;error=$null}
function Save-State {$state|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Encoding utf8NoBOM}
$mounted=$false
try{
    if(Get-VM -Name $name -ErrorAction SilentlyContinue){throw 'Lab VM already exists.'}
    if(Get-Volume -DriveLetter Z -ErrorAction SilentlyContinue){throw 'Temporary drive Z already exists.'}
    $vhd=Mount-VHD -Path $diskPath -PassThru;$mounted=$true;$disk=$vhd|Get-Disk
    $identity=Get-VHD -DiskNumber $disk.Number
    $parts=@(Get-Partition -DiskNumber $disk.Number)
    if($disk.IsBoot -or $disk.IsSystem -or $identity.Path -ne $diskPath -or $identity.Size -ne 32GB -or $parts.Count -ne 1 -or
        ($parts[0]|Get-Volume).FileSystemLabel -ne 'SHDD-Lab-OS' -or $parts[0].DriveLetter -ne 'Z'){throw 'Owned OS disk identity mismatch.'}
    $password='S!'+[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(20))+'a9'
    [PSCredential]::new('Administrator',(ConvertTo-SecureString $password -AsPlainText -Force))|Export-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    $unattend=@"
<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend"><settings pass="specialize"><component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><ComputerName>SHDD-LAB</ComputerName><TimeZone>UTC</TimeZone></component></settings><settings pass="oobeSystem"><component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><UserAccounts><AdministratorPassword><Value>$password</Value><PlainText>true</PlainText></AdministratorPassword></UserAccounts><OOBE><HideEULAPage>true</HideEULAPage><HideLocalAccountScreen>true</HideLocalAccountScreen><HideOnlineAccountScreens>true</HideOnlineAccountScreens><HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE><ProtectYourPC>3</ProtectYourPC><SkipMachineOOBE>true</SkipMachineOOBE><SkipUserOOBE>true</SkipUserOOBE></OOBE></component></settings></unattend>
"@
    New-Item -ItemType Directory -Path Z:\Windows\Panther -Force|Out-Null
    $unattend|Set-Content -LiteralPath Z:\Windows\Panther\unattend.xml -Encoding utf8NoBOM
    $password=$null;$unattend=$null
    Dismount-VHD -Path $diskPath;$mounted=$false
    $repair='cmd /c if exist C:\Windows\System32\ntoskrnl.exe (bcdboot C:\Windows /s C: /f BIOS /c > C:\boot-repair.log 2>&1 && D:\boot\bootsect.exe /nt60 C: /mbr /force >> C:\boot-repair.log 2>&1 && wpeutil reboot)'
    $escaped=[Security.SecurityElement]::Escape($repair)
    $answer=@"
<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend"><settings pass="windowsPE"><component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><RunSynchronous><RunSynchronousCommand xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" wcm:action="add"><Order>1</Order><Path>$escaped</Path></RunSynchronousCommand></RunSynchronous></component></settings></unattend>
"@
    # Minimal FAT12 floppy with a standards-compliant VFAT long filename entry.
    $image=[byte[]]::new(1474560)
    function Put16([int]$offset,[uint16]$value){[BitConverter]::GetBytes($value).CopyTo($image,$offset)}
    $image[0]=0xeb;$image[1]=0x3c;$image[2]=0x90;[Text.Encoding]::ASCII.GetBytes('MSDOS5.0').CopyTo($image,3)
    Put16 11 512;$image[13]=1;Put16 14 1;$image[16]=2;Put16 17 224;Put16 19 2880;$image[21]=0xf0;Put16 22 9;Put16 24 18;Put16 26 2
    $image[38]=0x29;[Text.Encoding]::ASCII.GetBytes('SHDD LAB   ').CopyTo($image,43);[Text.Encoding]::ASCII.GetBytes('FAT12   ').CopyTo($image,54);$image[510]=0x55;$image[511]=0xaa
    $content=[Text.Encoding]::UTF8.GetBytes($answer);$clusters=[int][Math]::Ceiling($content.Length/512.0)
    $fat=[byte[]]::new(4608);$fat[0]=0xf0;$fat[1]=0xff;$fat[2]=0xff
    for($cluster=2;$cluster -lt 2+$clusters;$cluster++){
        $next=if($cluster -eq 1+$clusters){0xfff}else{$cluster+1};$offset=[int][Math]::Floor($cluster*1.5)
        if($cluster%2){$fat[$offset]=($fat[$offset]-band 0x0f)-bor(($next-band 0xf)-shl 4);$fat[$offset+1]=$next-shr 4}
        else{$fat[$offset]=$next-band 0xff;$fat[$offset+1]=($fat[$offset+1]-band 0xf0)-bor($next-shr 8)}
    }
    $fat.CopyTo($image,512);$fat.CopyTo($image,5120)
    $short=[Text.Encoding]::ASCII.GetBytes('AUTOUN~1XML');$checksum=0
    foreach($ch in $short){$checksum=((($checksum-band 1)-shl 7)+($checksum-shr 1)+$ch)-band 255}
    $long='Autounattend.xml';$positions=@(1,3,5,7,9,14,16,18,20,22,24,28,30);$rootOffset=9728
    foreach($sequence in 2,1){
        $entry=$rootOffset+(2-$sequence)*32;$image[$entry]=$sequence-bor $(if($sequence -eq 2){0x40}else{0});$image[$entry+11]=0x0f;$image[$entry+13]=$checksum
        for($j=0;$j -lt 13;$j++){$index=($sequence-1)*13+$j;$value=if($index -lt $long.Length){[int][char]$long[$index]}elseif($index -eq $long.Length){0}else{0xffff};Put16 ($entry+$positions[$j]) $value}
    }
    $entry=$rootOffset+64;$short.CopyTo($image,$entry);$image[$entry+11]=0x20;Put16 ($entry+26) 2
    [BitConverter]::GetBytes([uint32]$content.Length).CopyTo($image,$entry+28);$content.CopyTo($image,16896)
    $floppy=Join-Path $lab 'repair-answer.vfd';[IO.File]::WriteAllBytes($floppy,$image)
    $vm=New-VM -Name $name -Generation 1 -MemoryStartupBytes 2GB -VHDPath $diskPath -Path $lab
    $vm|Get-VMNetworkAdapter|Remove-VMNetworkAdapter
    Set-VM -VM $vm -ProcessorCount 2 -AutomaticCheckpointsEnabled $false -AutomaticStartAction Nothing -AutomaticStopAction ShutDown
    Set-VMMemory -VM $vm -DynamicMemoryEnabled $false
    Enable-VMIntegrationService -VM $vm -Name 'Guest Service Interface'
    Add-VMDvdDrive -VM $vm -Path (Join-Path $lab 'windows-server-2022-eval.iso')
    Set-VMFloppyDiskDrive -VM $vm -Path $floppy
    Set-VMBios -VM $vm -StartupOrder CD,IDE,Floppy,LegacyNetworkAdapter
    $state.vmId=$vm.Id.ToString();$state.generation=1;$state.memoryBytes=2GB;$state.osDisk=$diskPath;$state.phase='booting-winpe-repair';Save-State
    Start-VM -VM $vm
    Start-Sleep -Seconds 3
    $system=Get-CimInstance -Namespace root\virtualization\v2 -ClassName Msvm_ComputerSystem -Filter ("Name='"+$vm.Id.ToString()+"'")
    $keyboard=Get-CimAssociatedInstance -InputObject $system -ResultClassName Msvm_Keyboard
    Invoke-CimMethod -InputObject $keyboard -MethodName TypeKey -Arguments @{keyCode=[uint32]13}|Out-Null
    Save-State
}catch{$state.phase='failed';$state.error=$_.Exception.Message;Save-State;throw}finally{if($mounted){Dismount-VHD -Path $diskPath -ErrorAction Continue}}
