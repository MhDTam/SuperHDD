$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$headers=Join-Path $root '.tools\dokan-sdk\2.3.1'
foreach($name in @('dokan.h','fileinfo.h','public.h')){if(-not(Test-Path -LiteralPath (Join-Path $headers $name))){throw 'Fetch the official v2.3.1.1000 headers before building this optional probe.'}}
$vs=& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$compilerRoot=(Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory|Sort-Object Name -Descending|Select-Object -First 1).FullName
$sdk='C:\Program Files (x86)\Windows Kits\10'
$version='10.0.26100.0'
$output=Join-Path $root 'artifacts\dokan-probe'
New-Item -ItemType Directory -Path $output -Force|Out-Null
$env:INCLUDE=(@((Join-Path $compilerRoot 'include'),(Join-Path $sdk "Include\$version\ucrt"),(Join-Path $sdk "Include\$version\shared"),(Join-Path $sdk "Include\$version\um")) -join ';')
$env:LIB=(@((Join-Path $compilerRoot 'lib\x64'),(Join-Path $sdk "Lib\$version\ucrt\x64"),(Join-Path $sdk "Lib\$version\um\x64")) -join ';')
& (Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe') /nologo /std:c++17 /LD /MT /EHsc /W4 /WX /DUNICODE /D_UNICODE ('/I'+$headers) ('/Fo'+(Join-Path $output 'bridge.obj')) (Join-Path $root 'tools\dokan-probe\bridge.cpp') /link ('/OUT:'+(Join-Path $output 'bridge.dll')) ('/IMPLIB:'+(Join-Path $output 'bridge.lib'))
if($LASTEXITCODE){throw 'Dokan probe ABI bridge build failed.'}
# Build only. No installation, mounts, registry, firmware or boot changes.
