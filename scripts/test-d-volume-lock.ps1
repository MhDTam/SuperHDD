# Diagnostic only: briefly acquire/release the NTFS volume lock, never offline,
# dismount, read raw sectors, change partitions or format any volume.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$disk=Get-Partition -DriveLetter D|Get-Disk
if($disk.Number -ne 2 -or $disk.IsBoot -or $disk.IsSystem -or $disk.UniqueId -ne '5000CC2A22D4D7E0' -or $disk.IsOffline){throw 'Reviewed online data disk D is required.'}
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class DVolumeLockDiagnostic {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string name,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool DeviceIoControl(SafeFileHandle file,uint code,IntPtr input,uint inBytes,IntPtr output,uint outBytes,out uint returned,IntPtr overlapped);
 public static int Check(){using(var file=CreateFileW(@"\\.\D:",0xc0000000,3,IntPtr.Zero,3,0,IntPtr.Zero)){if(file.IsInvalid)return Marshal.GetLastWin32Error();uint n;if(!DeviceIoControl(file,0x90018,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero))return Marshal.GetLastWin32Error();if(!DeviceIoControl(file,0x9001c,IntPtr.Zero,0,IntPtr.Zero,0,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return 0;}}
}
'@
$previous=Get-Service TrkWks;$stopped=$false;$attempts=@()
try{
    if($previous.Status -eq 'Running'){Stop-Service TrkWks;$stopped=$true;(Get-Service TrkWks).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(15))}
    Start-Sleep -Seconds 2
    for($i=0;$i -lt 10;$i++){
        $code=[DVolumeLockDiagnostic]::Check()
        $attempts+=,[ordered]@{at=[DateTime]::UtcNow.ToString('o');win32Error=$code;linkTracking=(Get-Service TrkWks).Status.ToString()}
        if($code -eq 0){break};Start-Sleep -Milliseconds 500
    }
}finally{
    if($stopped){Start-Service TrkWks}
    [ordered]@{attempts=$attempts;sourceOffline=(Get-Disk -Number 2).IsOffline;linkTrackingRestored=((Get-Service TrkWks).Status -eq $previous.Status)}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $root 'artifacts\diagnostics\d-lock-test.json') -Encoding utf8NoBOM
}
