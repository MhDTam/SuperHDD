param([string]$OutputDirectory='artifacts\loading-cache')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$headers=Join-Path $root '.tools\dokan-sdk\2.3.1'
$vs=& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$compilerRoot=(Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory|Sort-Object Name -Descending|Select-Object -First 1).FullName
$sdk='C:\Program Files (x86)\Windows Kits\10';$version='10.0.26100.0'
$output=[IO.Path]::GetFullPath((Join-Path $root $OutputDirectory))
if(-not $output.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Build output must stay in this workspace'}
New-Item -ItemType Directory -Path $output -Force|Out-Null
$env:INCLUDE=(@((Join-Path $compilerRoot 'include'),(Join-Path $sdk "Include\$version\ucrt"),(Join-Path $sdk "Include\$version\shared"),(Join-Path $sdk "Include\$version\um")) -join ';')
$env:LIB=(@((Join-Path $compilerRoot 'lib\x64'),(Join-Path $sdk "Lib\$version\ucrt\x64"),(Join-Path $sdk "Lib\$version\um\x64")) -join ';')
& (Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe') /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX /Zi /DUNICODE /D_UNICODE ('/I'+$headers) ('/Fo'+(Join-Path $output 'worker.obj')) ('/Fd'+(Join-Path $output 'worker-compile.pdb')) (Join-Path $root 'tools\loading-cache\worker.cpp') /link /DEBUG ('/OUT:'+(Join-Path $output 'folder-worker.exe')) advapi32.lib bcrypt.lib
if($LASTEXITCODE){throw 'Folder worker build failed.'}

& (Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe') /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX /DUNICODE /D_UNICODE ('/Fo'+(Join-Path $output 'pool-test.obj')) (Join-Path $root 'tools\loading-cache\pool-test.cpp') /link ('/OUT:'+(Join-Path $output 'pool-test.exe')) bcrypt.lib
if($LASTEXITCODE){throw 'Pool test build failed.'}

& (Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe') /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX ('/Fo'+(Join-Path $output 'read-load.obj')) (Join-Path $root 'tools\loading-cache\read-load.cpp') /link ('/OUT:'+(Join-Path $output 'read-load.exe'))
if($LASTEXITCODE){throw 'Read load test build failed.'}

foreach($variant in @('baseline','candidate')){
 $definitions=@();if($variant -eq 'baseline'){$definitions=@('/DBASELINE_POOL')}
 & (Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe') /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX @definitions ('/Fo'+(Join-Path $output ('pool-bench-'+$variant+'.obj'))) (Join-Path $root 'tools\loading-cache\pool-bench.cpp') /link ('/OUT:'+(Join-Path $output ('pool-bench-'+$variant+'.exe'))) bcrypt.lib
 if($LASTEXITCODE){throw ('Pool benchmark build failed: '+$variant)}
}
