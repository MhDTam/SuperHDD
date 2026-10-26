$ErrorActionPreference='Stop'
$outputRoot=Join-Path (Get-Location) 'artifacts\pubg-diagnostics'
[void][IO.Directory]::CreateDirectory($outputRoot)
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class GameImageProbe {
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
 static extern SafeFileHandle CreateFileW(string path, uint access, uint share, IntPtr security, uint creation, uint flags, IntPtr template);
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
 static extern IntPtr CreateFileMappingW(SafeFileHandle file, IntPtr security, uint protection, uint high, uint low, string name);
 [DllImport("kernel32.dll", SetLastError=true)]
 static extern IntPtr MapViewOfFile(IntPtr mapping, uint access, uint high, uint low, UIntPtr size);
 [DllImport("kernel32.dll")] static extern bool UnmapViewOfFile(IntPtr address);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static object Probe(string path, bool image, bool notify) {
  var watch=Stopwatch.StartNew(); int error=0; string step="open"; bool ok=false;
  using(var file=CreateFileW(path,0x80000000,7,IntPtr.Zero,3,0,IntPtr.Zero)) {
   if(file.IsInvalid) error=Marshal.GetLastWin32Error();
   else {
    step="createMapping";
    uint protection=image ? (notify?0x1000002u:0x11000002u) : 2u;
    IntPtr mapping=CreateFileMappingW(file,IntPtr.Zero,protection,0,0,null);
    if(mapping==IntPtr.Zero)error=Marshal.GetLastWin32Error();
    else try {
     step="mapView";IntPtr view=MapViewOfFile(mapping,4,0,0,new UIntPtr(4096));
     if(view==IntPtr.Zero)error=Marshal.GetLastWin32Error();
     else try{byte first=Marshal.ReadByte(view);ok=first==0x4d;}finally{UnmapViewOfFile(view);}
    }finally{CloseHandle(mapping);}
   }
  }
  return new {path=path,image=image,imageNotifications=notify,ok=ok,step=step,error=error,message=error==0?null:new Win32Exception(error).Message,milliseconds=watch.Elapsed.TotalMilliseconds};
 }
}
'@
$journal=Get-Content -Raw 'artifacts\library-runtime\1f7b37d343e5c2ade23506ab3e4e7853\current.json'|ConvertFrom-Json
$relative='TslGame\Binaries\Win64\TslGame.exe'
$results=@(foreach($root in @($journal.source,$journal.backing)){
 foreach($mode in @('data','imageNoExecute','image')){
  $result=[GameImageProbe]::Probe((Join-Path $root $relative),$mode -ne 'data',$mode -eq 'image')
  $result|ConvertTo-Json -Compress|Add-Content -Encoding UTF8 (Join-Path $outputRoot 'image-probes.jsonl')
  $result
 }
})
$results|ConvertTo-Json -Depth 5|Set-Content -Encoding UTF8 (Join-Path $outputRoot 'image-probes.json')
$results|ConvertTo-Json -Depth 5
