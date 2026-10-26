param([ValidateSet('active','restored')][string]$Mode='active',[string]$BaselinePath,[string]$JournalPath,[string]$ProofPath)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
if(-not $BaselinePath){$BaselinePath=Join-Path $workspace 'artifacts\diagnostics\sparking-zero-original-files.json'}
$baseline=Get-Content -LiteralPath $BaselinePath -Raw|ConvertFrom-Json
$source=$baseline.source
if($source -ne 'D:\GAMES\DRAGON BALL - Sparking! ZERO'){throw 'Unexpected source'}
if(-not $JournalPath){$JournalPath=Join-Path $workspace 'artifacts\folder-runtime\current.json'}
$journal=Get-Content -LiteralPath $JournalPath -Raw|ConvertFrom-Json
if($journal.source -ne $source){throw 'Unexpected session'}
$backing=if($Mode -eq 'active'){$journal.backing}else{$source}
if($Mode -eq 'active' -and $journal.phase -ne 'active'){throw 'Session is not active'}
if($Mode -eq 'restored' -and ($journal.phase -ne 'stopped' -or -not $journal.restored)){throw 'Source is not restored'}
foreach($root in @($source,$backing)|Select-Object -Unique){
 $files=@(Get-ChildItem -LiteralPath $root -File -Recurse -Force)
 if($files.Count -ne $baseline.files.Count){throw 'File count changed'}
 foreach($entry in $baseline.files){$file=Get-Item -LiteralPath (Join-Path $root $entry.relativePath) -Force;if($file.Length -ne $entry.length -or $file.LastWriteTimeUtc.Ticks -ne $entry.lastWriteUtcTicks -or [int]$file.Attributes -ne $entry.attributes){throw ('Metadata mismatch: '+$entry.relativePath)}}
}
$expected=if($baseline.launcherSha256){$baseline.launcherSha256}else{(Get-Content -LiteralPath (Join-Path $workspace 'artifacts\diagnostics\sparking-zero-launcher-sha256.txt') -Raw).Trim()}
if((Get-FileHash -LiteralPath (Join-Path $source 'SparkingZERO.exe')).Hash -ne $expected){throw 'Launcher hash changed'}
$proof=[ordered]@{mode=$Mode;sessionId=$journal.id;source=$source;fileCount=$baseline.files.Count;metadataUnchanged=$true;launcherSha256=$expected;sampledBytesEqual=$null;warmSsdReads=$null;boot=(& (Join-Path $PSScriptRoot 'local-boot-state.ps1'));passed=$false}
if($Mode -eq 'active'){
 Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class GameCacheRead {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)] static extern bool SetFilePointerEx(SafeFileHandle h,long o,IntPtr n,uint m);
 [DllImport("kernel32",SetLastError=true)] static extern bool ReadFile(SafeFileHandle h,IntPtr b,uint n,out uint done,IntPtr overlap);
 [DllImport("kernel32")] static extern IntPtr VirtualAlloc(IntPtr a,UIntPtr n,uint t,uint p);
 [DllImport("kernel32")] static extern bool VirtualFree(IntPtr a,UIntPtr n,uint t);
 public static byte[] Read(string path,long offset){using(var h=CreateFileW(path,0x80000000,7,IntPtr.Zero,3,0x20000000,IntPtr.Zero)){if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();IntPtr b=VirtualAlloc(IntPtr.Zero,(UIntPtr)65536,0x3000,4);if(b==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();try{uint done;if(!SetFilePointerEx(h,offset,IntPtr.Zero,0)||!ReadFile(h,b,65536,out done,IntPtr.Zero)||done!=65536)throw new System.IO.IOException("Read failed");byte[] value=new byte[done];Marshal.Copy(b,value,0,(int)done);return value;}finally{VirtualFree(b,UIntPtr.Zero,0x8000);}}}
 public static bool Equal(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
}
'@
 $samples=@();foreach($name in @('pakchunk0-Windows.pak','pakchunk0-Windows.ucas','pakchunk1-Windows.ucas')){foreach($offset in @(0,67108864,536870912)){$samples+=@{path=('SparkingZERO\Content\Paks\'+$name);offset=$offset}}}
 foreach($sample in $samples){$a=[GameCacheRead]::Read((Join-Path $source $sample.path),$sample.offset);$b=[GameCacheRead]::Read((Join-Path $backing $sample.path),$sample.offset);if(-not [GameCacheRead]::Equal($a,$b)){throw 'Sample byte mismatch'}}
 $status=Join-Path $journal.task 'status.json'
 Start-Sleep -Milliseconds 500
 $cold=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
 foreach($sample in $samples){[void][GameCacheRead]::Read((Join-Path $source $sample.path),$sample.offset)}
 Start-Sleep -Milliseconds 500
 $warm=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
 if($warm.hits -lt ($cold.hits+$samples.Count) -or $warm.ssdReadBytes -lt ($cold.ssdReadBytes+65536*$samples.Count) -or $warm.sourceReadBytes -ne $cold.sourceReadBytes){throw 'Repeated samples did not use SSD exclusively'}
 $proof.sampledBytesEqual=$true;$proof.warmSsdReads=$true;$proof.sampleCount=$samples.Count;$proof.cold=$cold;$proof.warm=$warm
}
if(-not $proof.boot.secureBootEnabled -or $proof.boot.testSigningActive){throw 'Unexpected boot policy'}
$proof.passed=$true
if(-not $ProofPath){$ProofPath=Join-Path $workspace ('artifacts\diagnostics\sparking-zero-'+$Mode+'-proof.json')}
$proof|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $ProofPath
[pscustomobject]$proof|Select-Object mode,sessionId,fileCount,metadataUnchanged,sampledBytesEqual,warmSsdReads,passed|ConvertTo-Json
