$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
$env:PSModulePath=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
$found=New-Object 'System.Collections.Generic.Dictionary[string,object]' ([StringComparer]::OrdinalIgnoreCase)
function Add-App([string]$name,[string]$path,[string]$origin,[string]$kind,[string[]]$icons=@()){
 if(-not $name -or -not $path){return}
 $path=[Environment]::ExpandEnvironmentVariables($path.Trim().Trim('"')).Replace('/','\').TrimEnd('\')
 if($path -notmatch '^[a-zA-Z]:\\[^\r\n"<>|?*]+$' -or $path.Length -le 3 -or $path -match '\\\.SuperHDD-' -or -not [IO.Directory]::Exists($path)){return}
 if(-not $found.ContainsKey($path)){$found[$path]=[ordered]@{name=$name.Trim();path=$path;origin=$origin;kind=$kind;iconDataUri=$null;iconPaths=@()}}elseif($origin -in @('Steam','Epic')){$found[$path].name=$name.Trim();$found[$path].kind='game';$found[$path].origin=$origin}
 $found[$path].iconPaths=@($found[$path].iconPaths)+@($icons|Where-Object {$_})
}
# Windows installer inventory; read both registry views, never run uninstallers.
foreach($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryHive]::LocalMachine)){
 foreach($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)){
  $base=$null;$key=$null
  try{$base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view);$key=$base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall');if(-not $key){continue}
   foreach($sub in $key.GetSubKeyNames()){$item=$null;try{$item=$key.OpenSubKey($sub);$name=[string]$item.GetValue('DisplayName');$path=[string]$item.GetValue('InstallLocation');$icon=[string]$item.GetValue('DisplayIcon');$iconFile=$icon;if($iconFile.StartsWith('"')){$iconFile=($iconFile.Split('"'))[1]}else{$iconFile=$iconFile -replace ',\s*-?\d+$',''};if(-not $path -and $iconFile -match '^[a-zA-Z]:\\.+\.exe$' -and [IO.File]::Exists($iconFile)){$path=[IO.Path]::GetDirectoryName($iconFile)};Add-App $name $path 'Windows' 'app' @($icon)}catch{}finally{if($item){$item.Dispose()}}}
  }catch{}finally{if($key){$key.Dispose()};if($base){$base.Dispose()}}
 }
}
# Steam's local library metadata. Missing clients/libraries are normal.
$steam=@();try{$key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Valve\Steam');if($key){$steam+=@([string]$key.GetValue('SteamPath'));$key.Dispose()}}catch{}
$libraries=New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
foreach($root in $steam){if(-not $root){continue};[void]$libraries.Add($root);$vdf=Join-Path $root 'steamapps\libraryfolders.vdf';if([IO.File]::Exists($vdf)){foreach($m in [regex]::Matches([IO.File]::ReadAllText($vdf),'"path"\s*"([^"\r\n]+)"')){[void]$libraries.Add($m.Groups[1].Value.Replace('\\','\'))}}}
foreach($library in $libraries){$apps=Join-Path $library 'steamapps';if(-not [IO.Directory]::Exists($apps)){continue};foreach($manifest in [IO.Directory]::EnumerateFiles($apps,'appmanifest_*.acf')){try{$vdf=[IO.File]::ReadAllText($manifest);$name=[regex]::Match($vdf,'"name"\s*"([^"\r\n]+)"').Groups[1].Value;$dir=[regex]::Match($vdf,'"installdir"\s*"([^"\r\n]+)"').Groups[1].Value;$appId=[regex]::Match($vdf,'"appid"\s*"(\d+)"').Groups[1].Value;$icons=@();if($appId){foreach($client in $steam){$cache=Join-Path $client 'appcache\librarycache';$icons+=Join-Path $cache ($appId+'_icon.jpg');$iconDir=Join-Path $cache $appId;if([IO.Directory]::Exists($iconDir)){$icons+=@([IO.Directory]::EnumerateFiles($iconDir,'*icon*')|Select-Object -First 8)}}};if($dir -and $dir -notmatch '[\\/:]'){Add-App $name (Join-Path (Join-Path $apps 'common') $dir) 'Steam' 'game' $icons}}catch{}}}
$epic=Join-Path $env:ProgramData 'Epic\EpicGamesLauncher\Data\Manifests'
if([IO.Directory]::Exists($epic)){foreach($manifest in [IO.Directory]::EnumerateFiles($epic,'*.item')){try{$item=[IO.File]::ReadAllText($manifest)|ConvertFrom-Json;$icons=@();if($item.LaunchExecutable){$icons+=Join-Path $item.InstallLocation $item.LaunchExecutable};Add-App $item.DisplayName $item.InstallLocation 'Epic' 'game' $icons}catch{}}}
# Folder-installed games that have no launcher/installer registration.
foreach($drive in [IO.DriveInfo]::GetDrives()){if($drive.DriveType -ne 'Fixed' -or -not $drive.IsReady){continue};$games=Join-Path $drive.RootDirectory.FullName 'GAMES';if([IO.Directory]::Exists($games)){foreach($path in [IO.Directory]::EnumerateDirectories($games)){if([IO.Path]::GetFileName($path) -ne 'steamapps' -and -not [IO.Directory]::Exists((Join-Path $path 'steamapps'))){Add-App ([IO.Path]::GetFileName($path)) $path 'Games folder' 'game'}}}}
# Include saved manual folders, even outside the launcher and GAMES inventories.
try{foreach($app in ($env:SUPERHDD_LIBRARY_PROFILES|ConvertFrom-Json)){Add-App $app.name $app.path 'Manual' 'app'}}catch{}
$result=@(foreach($app in ($found.Values|Sort-Object @{Expression={if($_.path -like 'D:\*'){0}else{1}}},name)){
 try{$app.iconDataUri=Get-LibraryIcon $app.path $app.iconPaths ($app.kind -eq 'game')}catch{}
 [pscustomobject]@{name=$app.name;path=$app.path;origin=$app.origin;kind=$app.kind;iconDataUri=$app.iconDataUri}
})
ConvertTo-Json -InputObject $result -Compress -Depth 4
