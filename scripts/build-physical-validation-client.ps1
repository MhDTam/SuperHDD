# Build a one-run client restricted to ONE new GUID-named D file. No raw-device
# API or supplied path is accepted. Reuses the VHD test's unbuffered byte oracle.
param([Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Run)
$ErrorActionPreference='Stop'
if($Run.Length -ne 32){throw 'One exact validation nonce required.'}
$root=Split-Path -Parent $PSScriptRoot
$out=Join-Path $root ('artifacts\runtime\physical-validation-'+$Run)
if(Test-Path -LiteralPath $out){throw 'Validation directory already exists; preserved.'}
$null=New-Item -ItemType Directory -Path $out
$original=Get-Content -LiteralPath (Join-Path $root 'tools\iscsi-cache-probe\Probe.cs') -Raw
$begin=$original.IndexOf('internal static class NativeFile')
$end=$original.IndexOf('internal static class Program',$begin)
if($begin -lt 0 -or $end -le $begin){throw 'Expected bounded client source absent.'}
$client=$original.Substring($begin,$end-$begin)
$fixture='D:\SuperHDD-native-validation-'+$Run+'.bin'
$literal={param([string]$value)'@"'+$value.Replace('"','""')+'"'}
$fileLiteral=& $literal $fixture;$outLiteral=& $literal $out
if(-not $client.Contains('if(path != "S:\\fixture.bin")')){throw 'Original fixture guard changed.'}
$client=$client.Replace('"S:\\fixture.bin"',$fileLiteral).Replace('GetDiskFreeSpace("S:\\",','GetDiskFreeSpace("D:\\",')
$prefix='using System;using System.IO;using System.Text;using System.Diagnostics;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;using FILETIME=System.Runtime.InteropServices.ComTypes.FILETIME;'
$program=@"
internal static class PhysicalValidation {
 const string PathName=$fileLiteral,Root=$outLiteral;
 [StructLayout(LayoutKind.Sequential)] struct Info { public uint Attributes;public FILETIME Created,Access,Write;public uint Volume,SizeHigh,SizeLow,Links,IndexHigh,IndexLow; }
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool GetFileInformationByHandle(SafeFileHandle file,out Info info);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint create,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool SetFileInformationByHandle(SafeFileHandle file,int kind,byte[] data,uint length);
 static string Key(SafeFileHandle file){Info i;if(!GetFileInformationByHandle(file,out i))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());if(i.Links!=1||i.SizeHigh!=0||i.SizeLow!=16777216||(i.Attributes&0x410)!=0)throw new IOException("Owned regular 16 MiB fixture required");return i.Volume.ToString("X8")+i.IndexHigh.ToString("X8")+i.IndexLow.ToString("X8")+i.Created.dwHighDateTime.ToString("X8")+i.Created.dwLowDateTime.ToString("X8");}
 static int Main(string[] args){try{
  if(args.Length!=2||args[1].Length>40||!System.Text.RegularExpressions.Regex.IsMatch(args[1],"^[a-z0-9-]+$"))throw new ArgumentException("One mode and report tag required");
  string mode=args[0],owner=System.IO.Path.Combine(Root,"owner.txt"),report=System.IO.Path.Combine(Root,args[1]+".json");
  if(mode=="seed"){if(File.Exists(owner))throw new IOException("Owner already exists");NativeFile.Run(mode,PathName,report);using(FileStream pin=new FileStream(PathName,FileMode.Open,FileAccess.Read,FileShare.ReadWrite)){using(FileStream record=new FileStream(owner,FileMode.CreateNew,FileAccess.Write,FileShare.Read)){byte[] bytes=Encoding.ASCII.GetBytes(Key(pin.SafeFileHandle));record.Write(bytes,0,bytes.Length);record.Flush(true);}}}
  else if(mode=="delete"){using(SafeFileHandle pin=CreateFileW(PathName,0x10080,3,IntPtr.Zero,3,0x200000,IntPtr.Zero)){if(pin.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());if(Key(pin)!=File.ReadAllText(owner))throw new IOException("Fixture identity changed; preserved");if(!SetFileInformationByHandle(pin,4,new byte[]{1},1))throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());}File.WriteAllText(report,"{\"passed\":true,\"ownedFixtureRemoved\":true}");}
  else {if(mode!="read"&&mode!="read-mutated"&&mode!="read-all-mutated"&&mode!="mutate"&&mode!="extents")throw new ArgumentException("Unknown validation mode");using(FileStream pin=new FileStream(PathName,FileMode.Open,FileAccess.Read,FileShare.ReadWrite)){if(Key(pin.SafeFileHandle)!=File.ReadAllText(owner))throw new IOException("Fixture identity changed; preserved");NativeFile.Run(mode,PathName,report);}}
  return 0;
 }catch(Exception error){Console.Error.WriteLine(error);return 1;}}
}
"@
$source=Join-Path $out 'PhysicalValidation.cs'
[IO.File]::WriteAllText($source,$prefix+[Environment]::NewLine+$client+[Environment]::NewLine+$program)
$exe=Join-Path $out 'PhysicalValidation.exe'
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /warnaserror+ /platform:x64 /target:exe ('/out:'+$exe) $source
if($LASTEXITCODE){throw 'Owned physical-file validation client did not compile.'}
[ordered]@{runId=$Run;fixture=$fixture;directory=$out;client=$exe;clientHash=(Get-FileHash -LiteralPath $exe).Hash;scope='One new owned 16 MiB NTFS file only; no raw-device APIs'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $out 'build.json') -Encoding utf8NoBOM
