# Restore the known pre-test Stopped/Disabled state only if no initiator resources
# or native-loopback owners remain. Never remove a target, portal or other owner.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$r=[ordered]@{at=[DateTime]::UtcNow.ToString('o');restored=$false;reason=$null}
try{
    if((Test-Path 'C:\ProgramData\SuperHDDLoopback\manifest.json') -or (Get-Service SuperHDDLoopbackLab,SuperHDDLoopbackService -ErrorAction SilentlyContinue)){throw 'A native cache owner remains; initiator preserved.'}
    $service=Get-Service MSiSCSI
    if($service.Status -eq 'Stopped' -and $service.StartType -eq 'Disabled'){$r.restored=$true;return}
    if($service.Status -ne 'Running'){throw 'Initiator is transitioning or already stopped with another startup policy; preserved.'}
    if(@(Get-IscsiSession).Count -or @(Get-IscsiTarget).Count -or @(Get-IscsiTargetPortal).Count -or @($service.DependentServices|Where-Object Status -eq 'Running').Count){throw 'Initiator resources or dependents remain; preserved.'}
    Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class InitiatorResourceCounts {
 [DllImport("iscsidsc.dll",CharSet=CharSet.Unicode)]static extern uint ReportIScsiPersistentLoginsW(out uint count,IntPtr buffer,ref uint bytes);
 [DllImport("iscsidsc.dll",CharSet=CharSet.Unicode)]static extern uint ReportIScsiSendTargetPortalsExW(out uint count,ref uint bytes,IntPtr buffer);
 static bool Empty(bool portal){uint count,bytes=0;uint code=portal?ReportIScsiSendTargetPortalsExW(out count,ref bytes,IntPtr.Zero):ReportIScsiPersistentLoginsW(out count,IntPtr.Zero,ref bytes);if(code==0)return count==0;if(code!=122||bytes>1048576)return false;bytes=Math.Max(bytes,1);IntPtr buffer=Marshal.AllocHGlobal((int)bytes);try{code=portal?ReportIScsiSendTargetPortalsExW(out count,ref bytes,buffer):ReportIScsiPersistentLoginsW(out count,buffer,ref bytes);return code==0&&count==0;}finally{Marshal.FreeHGlobal(buffer);}}
 public static bool Empty(){return Empty(true)&&Empty(false);}
}
'@
    if(-not [InitiatorResourceCounts]::Empty()){throw 'Persistent/discovery resources could not be proven empty; preserved.'}
    Stop-Service MSiSCSI
    (Get-Service MSiSCSI).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(20))
    Set-Service MSiSCSI -StartupType Disabled
    $r.restored=((Get-Service MSiSCSI).Status -eq 'Stopped' -and (Get-Service MSiSCSI).StartType -eq 'Disabled')
}catch{$r.reason=$_.Exception.Message}finally{$r|ConvertTo-Json|Set-Content (Join-Path $root 'artifacts\diagnostics\native-initiator-restoration.json') -Encoding utf8NoBOM}
