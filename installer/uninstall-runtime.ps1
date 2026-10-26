$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Runtime removal requires Windows administrator rights.'}
$root=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Super HDD Runtime'
$absolute=[IO.Path]::GetFullPath($root)
if([IO.Path]::GetFileName($absolute) -ne 'Super HDD Runtime' -or [IO.Path]::GetDirectoryName($absolute) -ne [IO.Path]::GetFullPath([Environment]::GetFolderPath('ProgramFiles'))){throw 'Unexpected runtime root.'}
$manifest=Get-Content -LiteralPath (Join-Path $root 'runtime-manifest.json') -Raw|ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.protocolVersion -ne 5){throw 'Unknown installed runtime.'}
foreach($entry in @(@{file='SuperHDDCacheDriver.sys';hash=$manifest.driverSha256},@{file='SuperHDDService.exe';hash=$manifest.serviceSha256})){
    if((Get-FileHash -LiteralPath (Join-Path $root $entry.file)).Hash -ne $entry.hash){throw 'Installed payload differs; preserving it for review.'}
}
$service=Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"
$driver=Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheDriver'"
if($service -or $driver -or (Test-Path 'C:\ProgramData\SuperHDDDirect')){
    $json='{"operation":"remove","request":null}'
    $encoded=([Text.Encoding]::UTF8.GetBytes($json)|ForEach-Object {$_.ToString('x2')}) -join ''
    & (Join-Path $root 'SuperHDDService.exe') --cache-operation $encoded
    if($LASTEXITCODE){throw 'Safe detach/removal did not complete; runtime files were preserved.'}
}
if((Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'") -or (Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheDriver'") -or (Test-Path 'C:\ProgramData\SuperHDDDirect')){throw 'Owned registration/state remains; preserving binaries.'}
$expected=@('SuperHDDCacheDriver.sys','SuperHDDService.exe','runtime-manifest.json')
if(@(Get-ChildItem -LiteralPath $root -Force|Where-Object Name -notin $expected).Count){throw 'Unexpected files in runtime directory; preserving them.'}
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class SuperHDDRemoval{[DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]public static extern bool MoveFileEx(string oldName,string newName,uint flags);}'
$restart=$false
foreach($name in $expected){
    $path=Join-Path $root $name
    try{Remove-Item -LiteralPath $path}catch{if(-not[SuperHDDRemoval]::MoveFileEx($path,$null,4)){throw};$restart=$true}
}
if($restart){if(-not[SuperHDDRemoval]::MoveFileEx($root,$null,4)){throw 'Cannot schedule removal of the empty runtime directory.'}}else{Remove-Item -LiteralPath $root}
[pscustomobject]@{removed=$true;restartRequired=$restart;sourceDataDeleted=$false}
