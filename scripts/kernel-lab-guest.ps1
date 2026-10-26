param([string]$Action,[string]$VmId)
$ErrorActionPreference='Stop'
$computer=Get-CimInstance Win32_ComputerSystem
if($computer.Manufacturer -ne 'Microsoft Corporation' -or $computer.Model -ne 'Virtual Machine' -or $env:COMPUTERNAME -ne 'SHDD-LAB'){
    throw 'This routine runs only in the disposable Super HDD lab guest.'
}
$lab='C:\SuperHDDLab'
New-Item -ItemType Directory -Path $lab -Force|Out-Null
$marker=Join-Path $lab 'vm-identity.txt'
if(Test-Path -LiteralPath $marker){if((Get-Content -LiteralPath $marker -Raw).Trim() -ne $VmId){throw 'Guest lab instance mismatch.'}}else{$VmId|Set-Content -LiteralPath $marker}
if($Action -eq 'Status'){
    $os=Get-CimInstance Win32_OperatingSystem
    $administrator=([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    [pscustomobject]@{computer=$env:COMPUTERNAME;os=$os.Caption;build=$os.BuildNumber;guestInstance=$VmId;uptimeSeconds=((Get-Date)-$os.LastBootUpTime).TotalSeconds;administrator=$administrator;files=@(Get-ChildItem -LiteralPath $lab|Select-Object Name,Length)}
    return
}
if($Action -eq 'Sign'){
    'certificate'|Set-Content -LiteralPath (Join-Path $lab 'sign-stage.txt')
    $subject='CN=SuperHDD Disposable VM '+$VmId
    $certificate=Get-ChildItem Cert:\LocalMachine\My|Where-Object {$_.Subject -eq $subject -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date).AddDays(1)}|Select-Object -First 1
    if(-not $certificate){$certificate=New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject -CertStoreLocation Cert:\LocalMachine\My -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddDays(14)}
    Export-Certificate -Cert $certificate -FilePath (Join-Path $lab 'lab-public.cer') -Force|Out-Null
    Import-Certificate -FilePath (Join-Path $lab 'lab-public.cer') -CertStoreLocation Cert:\LocalMachine\Root|Out-Null
    # A pristine Server Core guest may not have this certificate store yet.
    # Create/open it through the normal certificate-store API; no ACL changes.
    $publishers=[Security.Cryptography.X509Certificates.X509Store]::new([Security.Cryptography.X509Certificates.StoreName]::TrustedPublisher,[Security.Cryptography.X509Certificates.StoreLocation]::LocalMachine)
    try{$publishers.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)}finally{$publishers.Close()}
    Import-Certificate -FilePath (Join-Path $lab 'lab-public.cer') -CertStoreLocation Cert:\LocalMachine\TrustedPublisher|Out-Null
    'signing'|Set-Content -LiteralPath (Join-Path $lab 'sign-stage.txt')
    & (Join-Path $lab 'signtool.exe') sign /v /ph /fd SHA256 /sm /s My /sha1 $certificate.Thumbprint (Join-Path $lab 'driver.sys') *> (Join-Path $lab 'sign.log')
    if($LASTEXITCODE){throw 'Guest driver test-signing failed.'}
    $signature=Get-AuthenticodeSignature -LiteralPath (Join-Path $lab 'driver.sys')
    if($signature.Status -ne 'Valid'){throw 'Guest driver test signature did not verify.'}
    # Gen1 lab only. Host Secure Boot, host trust stores, and host boot policy
    # are never changed. The guest will still require a signed test binary.
    $boot=& bcdedit.exe /enum '{current}'
    $testMode=$LASTEXITCODE -eq 0 -and ($boot -match '^testsigning\s+Yes\s*$')
    if(-not $testMode){
        & bcdedit.exe /set testsigning on *> (Join-Path $lab 'test-signing.log')
        if($LASTEXITCODE){throw 'Could not enable standard test-signed-driver development in the guest.'}
    }
    [pscustomobject]@{signed=$true;signatureStatus=$signature.Status.ToString();certificateThumbprint=$certificate.Thumbprint;guestRebootScheduled=(-not $testMode);scope='Disposable guest only'}
    if(-not $testMode){& shutdown.exe /r /t 8 /f|Out-Null}
    return
}
if($Action -in 'PrepareVolumes','ResumeVolumes','Unload','Helpers'){
    if($Action -eq 'PrepareVolumes'){
    if(Test-Path -LiteralPath (Join-Path $lab 'source.vhdx')){throw 'Disposable volumes already exist; refusing replacement.'}
    if(Get-Volume -DriveLetter R,T -ErrorAction SilentlyContinue){throw 'Disposable drive letters are already in use.'}
    $commands=@"
create vdisk file="C:\SuperHDDLab\source.vhdx" maximum=256 type=expandable
select vdisk file="C:\SuperHDDLab\source.vhdx"
attach vdisk
create partition primary
format fs=ntfs quick label=SHDD-Lab-Source
assign letter=R
create vdisk file="C:\SuperHDDLab\cache.vhdx" maximum=1024 type=expandable
select vdisk file="C:\SuperHDDLab\cache.vhdx"
attach vdisk
create partition primary
assign letter=T
exit
"@
    $commands|Set-Content -LiteralPath (Join-Path $lab 'create-volumes.txt')
    & diskpart.exe /s (Join-Path $lab 'create-volumes.txt') *> (Join-Path $lab 'create-volumes.log')
    if($LASTEXITCODE){throw 'Guest disposable VHD creation failed.'}
    foreach($letter in 'R','T'){
        $disk=Get-Partition -DriveLetter $letter|Get-Disk
        if($disk.BusType -ne 'File Backed Virtual' -or $disk.IsBoot -or $disk.IsSystem){throw 'Guest volume is not a disposable data VHD.'}
    }
    }else{
        if(-not (Test-Path -LiteralPath (Join-Path $lab 'source.vhdx')) -or -not (Test-Path -LiteralPath (Join-Path $lab 'cache.vhdx'))){throw 'Owned guest VHDs are missing.'}
        foreach($owned in @(@{file='C:\SuperHDDLab\source.vhdx';letter='R';label='SHDD-Lab-Source'},@{file='C:\SuperHDDLab\cache.vhdx';letter='T';label=''})){
            if(Get-Volume -DriveLetter $owned.letter -ErrorAction SilentlyContinue){continue}
            $image=Get-DiskImage -ImagePath $owned.file
            if(-not $image.Attached){Mount-DiskImage -ImagePath $owned.file -NoDriveLetter|Out-Null}
            $disk=Get-DiskImage -ImagePath $owned.file|Get-Disk
            if($disk.IsBoot -or $disk.IsSystem -or $disk.BusType -ne 'File Backed Virtual'){throw 'Owned guest disk identity mismatch.'}
            $partition=@($disk|Get-Partition|Where-Object Type -ne 'Reserved')
            if($partition.Count -ne 1){throw 'Unexpected owned guest partition count.'}
            if($owned.label -and ($partition[0]|Get-Volume).FileSystemLabel -ne $owned.label){throw 'Owned guest source label mismatch.'}
            $partition[0]|Set-Partition -NewDriveLetter $owned.letter
        }
    }
    foreach($letter in 'R','T'){
        $disk=Get-Partition -DriveLetter $letter|Get-Disk
        if($disk.BusType -ne 'File Backed Virtual' -or $disk.IsBoot -or $disk.IsSystem){throw 'Guest volume is not a disposable data VHD.'}
    }
    if((Get-Volume -DriveLetter R).FileSystem -ne 'NTFS' -or (Get-Volume -DriveLetter T).FileSystem -notin 'RAW',''){throw 'Unexpected guest filesystem.'}
    if(-not (Test-Path -LiteralPath (Join-Path $lab 'marker.bin'))){
        & (Join-Path $lab 'raw_lab.exe') bootstrap (Join-Path $lab 'marker.bin')
        if($LASTEXITCODE){throw ('Marker creation failed: exit '+$LASTEXITCODE)}
    }
    Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class LabVolume {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sa,uint c,uint f,IntPtr t);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint ib,IntPtr o,uint ob,out uint n,IntPtr ov);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool WriteFile(SafeFileHandle h,byte[] b,uint n,out uint written,IntPtr ov);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool ReadFile(SafeFileHandle h,byte[] b,uint n,out uint read,IntPtr ov);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFilePointerEx(SafeFileHandle h,long distance,out long position,uint method);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint QueryDosDevice(string p,StringBuilder b,int n);
 public static string Device(string letter){var b=new StringBuilder(1024);if(QueryDosDevice(letter,b,b.Capacity)==0)throw new System.ComponentModel.Win32Exception();return b.ToString();}
 public static void ResetOwned(string path,byte[] marker){
  if(marker.Length!=4096)throw new Exception("Marker size");
  using(var h=CreateFile(path,0xC0000000,3,IntPtr.Zero,3,0,IntPtr.Zero)){
   if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();uint n;
   if(!DeviceIoControl(h,0x90018,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();
   try{
    var old=new byte[4096];if(!ReadFile(h,old,4096,out n,IntPtr.Zero)||n!=4096)throw new System.ComponentModel.Win32Exception();
    for(int i=0;i<8;i++)if(old[i]!=marker[i])throw new Exception("Unknown cache header");
    for(int i=56;i<72;i++)if(old[i]!=marker[i])throw new Exception("Cache instance differs from this lab");
    ulong stored=BitConverter.ToUInt64(old,72);for(int i=72;i<80;i++)old[i]=0;ulong hash=0xcbf29ce484222325;
    unchecked{for(int i=0;i<128;i++)hash=(hash^old[i])*0x100000001b3;}
    if(hash!=stored)throw new Exception("Invalid cache header checksum");
    long position;if(!SetFilePointerEx(h,0,out position,0))throw new System.ComponentModel.Win32Exception();
    if(!WriteFile(h,marker,4096,out n,IntPtr.Zero)||n!=4096)throw new System.ComponentModel.Win32Exception();
   }finally{DeviceIoControl(h,0x9001c,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero);}
  }
 }
 public static void Mark(string path,byte[] marker){if(marker.Length!=4096)throw new Exception("Marker size");using(var h=CreateFile(path,0xC0000000,3,IntPtr.Zero,3,0,IntPtr.Zero)){if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();uint n;if(!DeviceIoControl(h,0x90018,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();try{var old=new byte[4096];if(!ReadFile(h,old,4096,out n,IntPtr.Zero)||n!=4096)throw new System.ComponentModel.Win32Exception();bool zero=true,same=true;for(int i=0;i<4096;i++){zero&=old[i]==0;same&=old[i]==marker[i];}if(same)return;if(!zero)throw new Exception("Refusing to replace nonzero cache volume header");long position;if(!SetFilePointerEx(h,0,out position,0))throw new System.ComponentModel.Win32Exception();if(!WriteFile(h,marker,4096,out n,IntPtr.Zero)||n!=4096)throw new System.ComponentModel.Win32Exception();}finally{DeviceIoControl(h,0x9001c,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero);}}}
}
public static class LabPnp {
 [StructLayout(LayoutKind.Sequential)] struct Info {public uint Size;public Guid Class;public uint DevInst;public UIntPtr Reserved;}
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr SetupDiGetClassDevs(ref Guid g,string e,IntPtr w,uint flags);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiEnumDeviceInfo(IntPtr h,uint i,ref Info info);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiGetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,out uint type,byte[] b,uint n,out uint required);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,byte[] b,uint n);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr h);
 public static void Enroll(string pdo){var g=new Guid("71a27cdd-812a-11d0-bec7-08002be2092f");var h=SetupDiGetClassDevs(ref g,null,IntPtr.Zero,2);if(h==new IntPtr(-1))throw new System.ComponentModel.Win32Exception();try{for(uint index=0;;index++){var i=new Info{Size=(uint)Marshal.SizeOf(typeof(Info))};if(!SetupDiEnumDeviceInfo(h,index,ref i)){if(Marshal.GetLastWin32Error()==259)break;throw new System.ComponentModel.Win32Exception();}uint t,n;var b=new byte[4096];if(!SetupDiGetDeviceRegistryProperty(h,ref i,14,out t,b,(uint)b.Length,out n))continue;if(!String.Equals(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0'),pdo,StringComparison.OrdinalIgnoreCase))continue;Array.Clear(b,0,b.Length);if(SetupDiGetDeviceRegistryProperty(h,ref i,17,out t,b,(uint)b.Length,out n)){var old=Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0');if(old=="SuperHDDRawLab")return;if(old.Length!=0)throw new Exception("Unexpected existing source filter");}else if(Marshal.GetLastWin32Error()!=13)throw new System.ComponentModel.Win32Exception();var filter=Encoding.Unicode.GetBytes("SuperHDDRawLab\0\0");if(!SetupDiSetDeviceRegistryProperty(h,ref i,17,filter,(uint)filter.Length))throw new System.ComponentModel.Win32Exception();return;}throw new Exception("Source PDO not found");}finally{SetupDiDestroyDeviceInfoList(h);}}
 public static void Remove(string pdo){
  var g=new Guid("71a27cdd-812a-11d0-bec7-08002be2092f");var h=SetupDiGetClassDevs(ref g,null,IntPtr.Zero,2);
  if(h==new IntPtr(-1))throw new System.ComponentModel.Win32Exception();
  try{for(uint index=0;;index++){
   var i=new Info{Size=(uint)Marshal.SizeOf(typeof(Info))};
   if(!SetupDiEnumDeviceInfo(h,index,ref i)){if(Marshal.GetLastWin32Error()==259)break;throw new System.ComponentModel.Win32Exception();}
   uint t,n;var b=new byte[4096];
   if(!SetupDiGetDeviceRegistryProperty(h,ref i,14,out t,b,(uint)b.Length,out n))continue;
   if(!String.Equals(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0'),pdo,StringComparison.OrdinalIgnoreCase))continue;
   Array.Clear(b,0,b.Length);
   if(!SetupDiGetDeviceRegistryProperty(h,ref i,17,out t,b,(uint)b.Length,out n)){if(Marshal.GetLastWin32Error()==13)return;throw new System.ComponentModel.Win32Exception();}
   if(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0')!="SuperHDDRawLab")throw new Exception("Refusing to remove another source filter");
   if(!SetupDiSetDeviceRegistryProperty(h,ref i,17,null,0))throw new System.ComponentModel.Win32Exception();return;
  }throw new Exception("Source PDO not found");}finally{SetupDiDestroyDeviceInfoList(h);}
 }
}
'@
    if($Action -eq 'Helpers'){return}
    if($Action -eq 'Unload'){
        & (Join-Path $lab 'raw_lab.exe') disable *> (Join-Path $lab 'disable-before-update.log')
        if($LASTEXITCODE){throw 'Could not disable lab session before update.'}
        $device=[LabVolume]::Device('R:')
        $sourceNode=Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device}
        if(@($sourceNode).Count -ne 1){throw 'Cannot identify the owned source for update.'}
        [LabPnp]::Remove($device)
        & pnputil.exe /restart-device $sourceNode.InstanceId *> (Join-Path $lab 'remove-filter.log')
        if($LASTEXITCODE){throw 'Could not restart disposable source for filter removal.'}
        & sc.exe stop SuperHDDRawLab *> (Join-Path $lab 'service-stop.log')
        if($LASTEXITCODE -eq 1052){
            # The PnP driver still owns its control device. Use a normal guest
            # reboot; never force-unload an image which may remain referenced.
            [pscustomobject]@{unloaded=$false;rebootRequired=$true}
            & shutdown.exe /r /t 8 /f|Out-Null;return
        }
        if($LASTEXITCODE){throw 'Could not unload the lab driver; no replacement permitted.'}
        [pscustomobject]@{unloaded=$true};return
    }
    if(-not (Test-Path -LiteralPath (Join-Path $lab 'cache-marked.txt'))){
        [LabVolume]::Mark('\\.\T:',[IO.File]::ReadAllBytes((Join-Path $lab 'marker.bin')))
        $VmId|Set-Content -LiteralPath (Join-Path $lab 'cache-marked.txt')
    }
    $request=[ordered]@{sourceDevice=[LabVolume]::Device('R:');cacheDevice=[LabVolume]::Device('T:');capacityBytes=512MB;acknowledgedDisposableVhds=$true}
    $request|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $lab 'request.json') -Encoding ASCII
    # Register only the explicitly matched disposable source volume instance.
    $volume=Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $request.sourceDevice}
    if(@($volume).Count -ne 1 -or $volume.InstanceId -notlike 'STORAGE\Volume\*'){throw 'Cannot uniquely identify disposable source PDO.'}
    $enumKey='HKLM:\SYSTEM\CurrentControlSet\Enum\'+$volume.InstanceId
    $old=Get-ItemProperty -LiteralPath $enumKey -Name UpperFilters -ErrorAction SilentlyContinue
    if($old.UpperFilters -and (@($old.UpperFilters).Count -ne 1 -or $old.UpperFilters[0] -ne 'SuperHDDRawLab')){throw 'Disposable volume unexpectedly has other upper filters.'}
    if(-not (Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'")){
    & sc.exe create SuperHDDRawLab type= kernel start= demand binPath= C:\SuperHDDLab\driver.sys *> (Join-Path $lab 'service-create.log')
    if($LASTEXITCODE){throw 'Guest test service registration failed.'}
    }
    if((Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'").State -ne 'Running'){
    & sc.exe start SuperHDDRawLab *> (Join-Path $lab 'service-start.log')
    if($LASTEXITCODE){throw 'Guest test driver did not load; inspect signature/start log.'}
    }
    [LabPnp]::Enroll($request.sourceDevice)
    # Detach/attach gives VHD volumes a new instance ID and loses this exact
    # device's filter property. Restart the same disposable devnode instead.
    $sourceDiskId=(Get-Partition -DriveLetter R|Get-Disk).UniqueId
    & pnputil.exe /restart-device $volume.InstanceId *> (Join-Path $lab 'restart-source.log')
    if($LASTEXITCODE){throw 'Disposable source restart failed.'}
    if(-not(Get-Volume -DriveLetter R -ErrorAction SilentlyContinue)){
        $sourceDisk=@(Get-Disk|Where-Object UniqueId -eq $sourceDiskId)
        if($sourceDisk.Count -ne 1 -or $sourceDisk[0].IsBoot -or $sourceDisk[0].IsSystem){throw 'Source changed after restart.'}
        $partition=@($sourceDisk[0]|Get-Partition|Where-Object Type -ne 'Reserved')
        if($partition.Count -ne 1 -or ($partition[0]|Get-Volume).FileSystemLabel -ne 'SHDD-Lab-Source'){throw 'Source volume identity changed after restart.'}
        $partition[0]|Set-Partition -NewDriveLetter R
    }
    $request.sourceDevice=[LabVolume]::Device('R:')
    $request|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $lab 'request.json') -Encoding ASCII
    $status=& (Join-Path $lab 'raw_lab.exe') status
    if($LASTEXITCODE){throw 'Driver status failed after source enrollment.'}
    $status|Set-Content -LiteralPath (Join-Path $lab 'status-loaded.json') -Encoding UTF8
    if(($status|ConvertFrom-Json).volumeCount -lt 1){throw 'Source volume restarted but no PnP filter attachment was recorded.'}
    [pscustomobject]@{sourceInstance=$volume.InstanceId;status=($status|ConvertFrom-Json);request=$request}
    return
}
if($Action -eq 'Probe'){
    if(Test-Path -LiteralPath (Join-Path $lab 'probe.json')){Move-Item -LiteralPath (Join-Path $lab 'probe.json') -Destination (Join-Path $lab ('probe-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmss')+'.json'))}
    & (Join-Path $lab 'raw_lab.exe') configure (Join-Path $lab 'request.json') *> (Join-Path $lab 'configure.log')
    if($LASTEXITCODE){throw 'RAW session configure failed.'}
    & (Join-Path $lab 'pnp_probe.exe') --raw ('R:\superhdd-pnp-proof-'+[Guid]::NewGuid().ToString('N')) (Join-Path $lab 'probe.json') (Join-Path $lab 'request.json') *> (Join-Path $lab 'probe.log')
    if($LASTEXITCODE){throw 'Kernel ordinary-file proof failed.'}
    $proof=Get-Content -LiteralPath (Join-Path $lab 'probe.json') -Raw|ConvertFrom-Json
    $proof|Add-Member driverImageSha256 (Get-FileHash -LiteralPath (Join-Path $lab 'driver.sys')).Hash
    $proof|Add-Member driverSignatureStatus (Get-AuthenticodeSignature -LiteralPath (Join-Path $lab 'driver.sys')).Status.ToString()
    $proof|ConvertTo-Json -Depth 16|Set-Content -LiteralPath (Join-Path $lab 'probe.json') -Encoding UTF8
    $proof
    return
}
throw 'Unknown guest lab action.'
