$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
if(Get-Process -Name 'folder-worker' -ErrorAction SilentlyContinue){throw 'An active worker exists; defer fixture cleanup'}
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class LagFixtureMount {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)]static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint n,byte[] o,uint l,out uint done,IntPtr x);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool RemoveDirectoryW(string p);
 public static string Tag(string p){using(var h=CreateFileW(p,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){byte[] b=new byte[16384];uint n;if(h.IsInvalid||!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();if(BitConverter.ToUInt32(b,0)!=0xa0000003)throw new Exception("Unexpected mount tag");return Convert.ToBase64String(b,0,(int)n);}}
 public static void Remove(string p,string expected){if(Tag(p)!=expected)throw new Exception("Mount identity changed");if(!RemoveDirectoryW(p))throw new System.ComponentModel.Win32Exception();}
}
'@
$removed=[Collections.Generic.List[string]]::new()
function Remove-TestRoot([string]$path){
 $resolved=[IO.Path]::GetFullPath($path)
 if($resolved -ne $path -or $resolved -notmatch '^[DE]:\\SuperHDD-(FolderLab|ConfigLab|LatencyLab)-[a-f0-9]{32}$'){throw 'Unexpected fixture root'}
 if(-not(Test-Path -LiteralPath $resolved)){return}
 if((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Reparse fixture root; preserve'}
 if(@(Get-ChildItem -LiteralPath $resolved -Recurse -Force | Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'Reparse child; preserve fixture'}
 Remove-Item -LiteralPath $resolved -Recurse -Force
 $removed.Add($resolved)
}
$latencyIds=@('500ad5f87d934fc08d923eae98670fb1','331af1d29bec4ad5853bab7d28744bb8')
$reference='D:\SuperHDD-LatencyLab-331af1d29bec4ad5853bab7d28744bb8\backing\archive.bin'
$referenceHash=(Get-FileHash -LiteralPath $reference).Hash
foreach($id in $latencyIds){
 $root='D:\SuperHDD-LatencyLab-'+$id
 $proof=Get-Content -LiteralPath (Join-Path $workspace ('artifacts\loading-cache\latency-'+$id+'\result.json')) -Raw|ConvertFrom-Json
 if($proof.fixture -ne $root -or (Get-FileHash -LiteralPath (Join-Path $root 'backing\archive.bin')).Hash -ne $referenceHash){throw 'Latency fixture ownership mismatch'}
 $mount=Join-Path $root 'selected'
 if((Get-Item -LiteralPath $mount -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){$tag=[LagFixtureMount]::Tag($mount);[LagFixtureMount]::Remove($mount,$tag)}
 Remove-TestRoot $root
 $artifactRoot=Join-Path $workspace ('artifacts\loading-cache\latency-'+$id)
 foreach($file in @(Get-ChildItem -LiteralPath $artifactRoot -Recurse -File | Where-Object {$_.Name -in @('cache.bin','cache.bin.index')})){
  if(-not $file.FullName.StartsWith($artifactRoot+'\',[StringComparison]::OrdinalIgnoreCase) -or $file.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unexpected test cache file'}
  Remove-Item -LiteralPath $file.FullName;$removed.Add($file.FullName)
 }
}
foreach($id in @('a94825ac42224b0dbc9e7ec3508bb0e2','e4a4be766df04916b7debdbb9780892c')){
 $proof=Get-Content -LiteralPath (Join-Path $workspace ('artifacts\loading-cache\runs\'+$id+'\result.json')) -Raw|ConvertFrom-Json
 if(-not $proof.passed -or -not $proof.sourceRestored){throw 'Filesystem proof not restored'}
 Remove-TestRoot ('D:\SuperHDD-FolderLab-'+$id)
}
$sessions=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\session-proof.json') -Raw|ConvertFrom-Json
if(-not $sessions.passed -or $sessions.runId -ne '17d113f4d79f451cb21809d31b796ff1'){throw 'Unexpected session proof'}
foreach($letter in @('D','E')){Remove-TestRoot ($letter+':\SuperHDD-ConfigLab-'+$sessions.runId)}
$unitParent=Join-Path $workspace 'artifacts\loading-cache\parallel-candidate'
foreach($dir in @(Get-ChildItem -LiteralPath $unitParent -Directory | Where-Object Name -match '^unit-[a-f0-9]{32}$')){
 if($dir.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unexpected unit fixture'}
 foreach($name in @('pool.bin','pool.bin.index')){$file=Join-Path $dir.FullName $name;if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file;$removed.Add($file)}}
}
@{removed=$removed.ToArray();proofArtifactsPreserved=$true;gameDirectoryChanged=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\diagnostics\euro-lag-fixture-cleanup.json') -Encoding utf8
'Removed '+$removed.Count+' owned fixture roots/cache files; evidence and user files preserved.'
