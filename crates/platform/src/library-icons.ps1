# Read local image/PE resources only. Never execute a discovered game or installer.
Add-Type -AssemblyName System.Drawing
if(-not ('SuperHdd.LibraryIcon' -as [type])){
 Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace SuperHdd {
 public static class LibraryIcon {
  [DllImport("shell32.dll", CharSet=CharSet.Unicode)]
  public static extern uint ExtractIconExW(string file, int index, out IntPtr large, out IntPtr small, uint count);
  [DllImport("user32.dll")]
  public static extern bool DestroyIcon(IntPtr icon);
 }
}
'@
}
function ConvertTo-LibraryIcon([string]$reference){
 $image=$null;$bitmap=$null;$graphics=$null;$stream=$null;$icon=$null
 $large=[IntPtr]::Zero;$small=[IntPtr]::Zero
 try{
  $path=[Environment]::ExpandEnvironmentVariables($reference.Trim());$index=0
  if($path -match '^"([^"]+)"(?:\s*,\s*(-?\d+))?\s*$'){
   $path=$Matches[1];if($Matches[2]){$index=[int]$Matches[2]}
  }elseif($path -match '^(.*),\s*(-?\d+)\s*$'){$path=$Matches[1].Trim();$index=[int]$Matches[2]}
  if($path -notmatch '^[a-zA-Z]:\\' -or -not [IO.File]::Exists($path)){return}
  $extension=[IO.Path]::GetExtension($path).ToLowerInvariant()
  if($extension -in @('.png','.jpg','.jpeg','.bmp')){
   if(([IO.FileInfo]$path).Length -gt 8MB){return}
   $image=[Drawing.Image]::FromFile($path)
  }elseif($extension -in @('.exe','.dll','.ico')){
   $count=[SuperHdd.LibraryIcon]::ExtractIconExW($path,$index,[ref]$large,[ref]$small,1)
   if($count -eq 0 -or $count -eq [uint32]::MaxValue -or $large -eq [IntPtr]::Zero){return}
   $icon=[Drawing.Icon]::FromHandle($large);$image=$icon.ToBitmap()
  }else{return}
  $bitmap=New-Object Drawing.Bitmap(48,48)
  $graphics=[Drawing.Graphics]::FromImage($bitmap)
  $graphics.InterpolationMode=[Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $scale=[Math]::Min(48.0/$image.Width,48.0/$image.Height)
  $width=[int]($image.Width*$scale);$height=[int]($image.Height*$scale)
  $graphics.DrawImage($image,[int]((48-$width)/2),[int]((48-$height)/2),$width,$height)
  $stream=New-Object IO.MemoryStream
  $bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
  'data:image/png;base64,'+[Convert]::ToBase64String($stream.ToArray())
 }catch{
  # Missing, inaccessible or malformed resources must not hide the application.
 }finally{
  if($graphics){$graphics.Dispose()};if($bitmap){$bitmap.Dispose()};if($image){$image.Dispose()}
  if($icon){$icon.Dispose()};if($stream){$stream.Dispose()}
  if($large -ne [IntPtr]::Zero){[void][SuperHdd.LibraryIcon]::DestroyIcon($large)}
  if($small -ne [IntPtr]::Zero -and $small -ne $large){[void][SuperHdd.LibraryIcon]::DestroyIcon($small)}
 }
}
function Get-LibraryIcon([string]$path,[string[]]$references=@(),[bool]$game=$true){
 $excluded='(?i)(unins|uninstall|setup|installer|crash|redist|vcredist|dxsetup|report|cefsubprocess|unitycrashhandler|updater|bootstrapper|binkplay|anticheat)'
 foreach($reference in $references){if($reference -and (-not $game -or [IO.Path]::GetFileName($reference) -notmatch $excluded)){$uri=ConvertTo-LibraryIcon $reference;if($uri){return $uri}}}
 # Breadth-first, bounded search reaches bin/win_x64 and Binaries/Win64 without
 # walking asset trees, following junctions, or scanning the whole drive.
 $queue=New-Object 'System.Collections.Generic.Queue[object]'
 $queue.Enqueue(@{path=$path;depth=0});$visited=0;$examined=0;$attempts=0
 while($queue.Count -gt 0 -and $visited -lt 192 -and $examined -lt 4096 -and $attempts -lt 32){
  $dir=$queue.Dequeue();$visited++;$files=@();$children=@()
  try{
   foreach($entry in [IO.Directory]::EnumerateFileSystemEntries($dir.path)){
    $examined++;if($examined -gt 4096){break}
    try{
     $name=[IO.Path]::GetFileName($entry);$attributes=[IO.File]::GetAttributes($entry)
     if(($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){continue}
     if(($attributes -band [IO.FileAttributes]::Directory) -ne 0){
      if($dir.depth -lt 5 -and $name -notmatch '^(?i:content|assets|data|engine|support|_commonredist|_redist|redist|redistributables|directx|locales|movies|localization|mods|cache|nodvd|easyanticheat|epiconlineservices|\.egstore|\.git)$'){$children+=@{path=$entry;depth=$dir.depth+1}}
     }elseif([IO.Path]::GetExtension($entry) -in @('.exe','.ico') -and $name -notmatch $excluded){$files+=$entry}
    }catch{}
   }
  }catch{}
  foreach($file in ($files|Sort-Object @{Expression={if($_ -like '*.ico'){0}elseif($_ -match '(?i)(launcher|helper|update)'){2}else{1}}},@{Expression={$_}})){
   $attempts++;$uri=ConvertTo-LibraryIcon $file;if($uri){return $uri};if($attempts -ge 32){break}
  }
  foreach($child in ($children|Sort-Object @{Expression={if([IO.Path]::GetFileName($_.path) -match '^(?i:bin|binaries|win64|win_x64|x64)$'){0}else{1}}},@{Expression={$_.path}})){
   if($queue.Count+$visited -lt 192){$queue.Enqueue($child)}
  }
 }
}
