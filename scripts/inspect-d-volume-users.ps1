# Read-only D: handle inventory. Never closes handles or stops applications.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$exe=Join-Path $root 'artifacts\diagnostics\sysinternals-handle\handle64.exe'
$signature=Get-AuthenticodeSignature -LiteralPath $exe
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation'){throw 'Verified Microsoft diagnostic required.'}
Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class DVolumeDeviceName {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern uint QueryDosDeviceW(string name,StringBuilder output,uint count);
 public static string Read(){var value=new StringBuilder(4096);if(QueryDosDeviceW("D:",value,4096)==0)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return value.ToString();}
}
'@
$device=[DVolumeDeviceName]::Read()
$raw=@(& $exe -accepteula -nobanner -v $device 2>&1)
$csv=@($raw|Where-Object {$_ -match '^("?Process"?,|"|[^,]+,[0-9]+,)'})
$rows=@($csv|ConvertFrom-Csv)
$summary=@($rows|Where-Object {$_.PID -match '^[0-9]+$'}|Group-Object Process,PID|ForEach-Object {
    [ordered]@{process=$_.Group[0].Process;pid=[int]$_.Group[0].PID;openReferences=$_.Count;volumeReferences=@($_.Group|Where-Object {$_.Name -eq 'D:'}).Count}
})
[ordered]@{at=[DateTime]::UtcNow.ToString('o');readOnly=$true;source='Microsoft Sysinternals Handle';device=$device;columns=@($rows|Select-Object -First 1|ForEach-Object {$_.PSObject.Properties.Name});processes=$summary;diagnosticExitCode=$LASTEXITCODE}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $root 'artifacts\diagnostics\d-volume-users.json') -Encoding utf8NoBOM
