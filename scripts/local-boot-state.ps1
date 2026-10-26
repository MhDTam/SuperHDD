# Read-only. This file deliberately contains no BCD, firmware or trust-store writes.
$ErrorActionPreference='Stop'
if(-not('SuperHDDLocalBoot' -as [type])){
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class SuperHDDLocalBoot {
 [StructLayout(LayoutKind.Sequential)] struct CI {public uint Length;public uint Options;}
 [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int kind,ref CI info,int length,out int returned);
 public static uint CodeIntegrity(){var i=new CI{Length=8};int n;int s=NtQuerySystemInformation(103,ref i,8,out n);if(s<0)throw new InvalidOperationException("Cannot query active code integrity: "+s.ToString("X8"));return i.Options;}
}
'@
}
$value=Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\SecureBoot\State'
$machine=(Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Cryptography').MachineGuid.Trim().ToLowerInvariant()
$digest=[Security.Cryptography.SHA256]::Create()
try{$machineHash=([BitConverter]::ToString($digest.ComputeHash([Text.Encoding]::UTF8.GetBytes($machine)))).Replace('-','')}finally{$digest.Dispose()}
$options=[SuperHDDLocalBoot]::CodeIntegrity()
[pscustomobject]@{readOnly=$true;machineSha256=$machineHash;secureBootEnabled=($value.UEFISecureBootEnabled -eq 1);testSigningActive=(($options -band 2) -ne 0);codeIntegrityOptions=$options}
