param([Parameter(Mandatory=$true)][string]$VolumeGuid,[Parameter(Mandatory=$true)][uint32]$DiskNumber,[Parameter(Mandatory=$true)][string]$NativeDevice,[Parameter(Mandatory=$true)][ValidateSet('Install','Remove')][string]$Action)
$ErrorActionPreference='Stop'
if($NativeDevice -notmatch '^\\Device\\HarddiskVolume[0-9]+$' -or $VolumeGuid -notmatch '^\\\\\?\\Volume\{[a-fA-F0-9-]{36}\}\\$'){throw 'Invalid native source identity.'}
$volume=Get-Volume -UniqueId $VolumeGuid
$partition=Get-Partition -AccessPath $VolumeGuid
$disk=$partition|Get-Disk
if($disk.Number -ne $DiskNumber -or $partition.IsBoot -or $partition.IsSystem -or $disk.BusType -notin 'ATA','SATA' -or $volume.FileSystem -ne 'NTFS' -or $volume.HealthStatus -ne 'Healthy'){throw 'Source is not the selected healthy direct data volume.'}
$nodes=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $NativeDevice})
if($nodes.Count -ne 1 -or $nodes[0].InstanceId -notlike 'STORAGE\Volume\*'){throw 'Source volume devnode ambiguous.'}
Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class RuntimePnp {
 [StructLayout(LayoutKind.Sequential)] struct Info {public uint Size;public Guid Class;public uint DevInst;public UIntPtr Reserved;}
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr SetupDiGetClassDevs(ref Guid g,string e,IntPtr w,uint flags);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiEnumDeviceInfo(IntPtr h,uint i,ref Info info);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiGetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,out uint type,byte[] b,uint n,out uint required);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,byte[] b,uint n);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr h);
 public static void Enroll(string pdo){var g=new Guid("71a27cdd-812a-11d0-bec7-08002be2092f");var h=SetupDiGetClassDevs(ref g,null,IntPtr.Zero,2);if(h==new IntPtr(-1))throw new System.ComponentModel.Win32Exception();try{for(uint index=0;;index++){var i=new Info{Size=(uint)Marshal.SizeOf(typeof(Info))};if(!SetupDiEnumDeviceInfo(h,index,ref i)){if(Marshal.GetLastWin32Error()==259)break;throw new System.ComponentModel.Win32Exception();}uint t,n;var b=new byte[4096];if(!SetupDiGetDeviceRegistryProperty(h,ref i,14,out t,b,(uint)b.Length,out n))continue;if(!String.Equals(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0'),pdo,StringComparison.OrdinalIgnoreCase))continue;Array.Clear(b,0,b.Length);if(SetupDiGetDeviceRegistryProperty(h,ref i,17,out t,b,(uint)b.Length,out n)){var old=Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0');if(old=="SuperHDDCacheDriver")return;if(old.Length!=0)throw new Exception("Unexpected existing source filter");}else if(Marshal.GetLastWin32Error()!=13)throw new System.ComponentModel.Win32Exception();var filter=Encoding.Unicode.GetBytes("SuperHDDCacheDriver\0\0");if(!SetupDiSetDeviceRegistryProperty(h,ref i,17,filter,(uint)filter.Length))throw new System.ComponentModel.Win32Exception();return;}throw new Exception("Source PDO not found");}finally{SetupDiDestroyDeviceInfoList(h);}}
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
   if(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0')!="SuperHDDCacheDriver")throw new Exception("Refusing to remove another source filter");
   if(!SetupDiSetDeviceRegistryProperty(h,ref i,17,null,0))throw new System.ComponentModel.Win32Exception();return;
  }throw new Exception("Source PDO not found");}finally{SetupDiDestroyDeviceInfoList(h);}
 }
}
'@
if($Action -eq 'Install'){[RuntimePnp]::Enroll($NativeDevice)}else{[RuntimePnp]::Remove($NativeDevice)}
& pnputil.exe /restart-device $nodes[0].InstanceId
if($LASTEXITCODE -eq 3010){exit 10}
if($LASTEXITCODE){throw 'Windows could not restart this data volume; close its open files and retry.'}
if($Action -eq 'Remove'){
    $references=@(Get-PnpDevice -Class Volume|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data -contains 'SuperHDDCacheDriver'})
    if($references.Count){throw 'Other volumes still reference this driver; preserve its service and files.'}
}
