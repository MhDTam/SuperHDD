$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class GameProcessProbe {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]
 struct STARTUPINFO {public int cb;public string reserved;public string desktop;public string title;public uint x,y,xSize,ySize,xChars,yChars,fill,flags;public short show,reservedSize;public IntPtr reservedData,input,output,error;}
 [StructLayout(LayoutKind.Sequential)]struct PROCESSINFO {public IntPtr process,thread;public uint processId,threadId;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
 static extern bool CreateProcessW(string application,System.Text.StringBuilder command,IntPtr processSecurity,IntPtr threadSecurity,bool inherit,uint flags,IntPtr environment,string directory,ref STARTUPINFO startup,out PROCESSINFO process);
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool TerminateProcess(IntPtr process,uint exit);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 public static object Probe(string path){
  var startup=new STARTUPINFO();startup.cb=Marshal.SizeOf(startup);PROCESSINFO process;
  var watch=Stopwatch.StartNew();
  // CREATE_SUSPENDED: no application entry-point instructions are run. Never resume.
  bool ok=CreateProcessW(path,null,IntPtr.Zero,IntPtr.Zero,false,0x08000004,IntPtr.Zero,Path.GetDirectoryName(path),ref startup,out process);
  int error=ok?0:Marshal.GetLastWin32Error();bool terminated=false;
  if(ok)try{
   terminated=TerminateProcess(process.process,0);if(!terminated)throw new Win32Exception(Marshal.GetLastWin32Error());
   if(WaitForSingleObject(process.process,5000)!=0)throw new Exception("Owned suspended probe did not exit");
  }finally{CloseHandle(process.thread);CloseHandle(process.process);}
  return new{path=path,created=ok,error=error,message=error==0?null:new Win32Exception(error).Message,entryPointNeverResumed=true,ownedProbeTerminated=terminated,milliseconds=watch.Elapsed.TotalMilliseconds};
 }
}
'@
if(Get-Process -Name TslGame,TslGame_BE,TslGame_ZK,TslGame_UC -ErrorAction SilentlyContinue){throw 'Existing game process preserved; close it before this probe'}
$journal=Get-Content -Raw 'artifacts\library-runtime\1f7b37d343e5c2ade23506ab3e4e7853\current.json'|ConvertFrom-Json
$results=@(foreach($root in @($journal.source,$journal.backing)){
 $result=[GameProcessProbe]::Probe((Join-Path $root 'TslGame\Binaries\Win64\TslGame.exe'))
 $result|ConvertTo-Json -Compress|Add-Content -Encoding UTF8 'artifacts\pubg-diagnostics\process-probes.jsonl'
 $result
})
$results|ConvertTo-Json -Depth 4|Set-Content -Encoding UTF8 'artifacts\pubg-diagnostics\process-probes.json'
$results|ConvertTo-Json -Depth 4
