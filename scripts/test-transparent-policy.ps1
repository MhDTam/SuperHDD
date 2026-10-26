
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if(-not $vs){throw 'Visual Studio C++ tools are required.'}
$compilerRoot=(Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
$sdkRoot='C:\Program Files (x86)\Windows Kits\10'
$version='10.0.26100.0'
$output=Join-Path $root 'artifacts\transparent-policy-test'
New-Item -ItemType Directory -Path $output -Force | Out-Null
$includes=@((Join-Path $compilerRoot 'include'),(Join-Path $sdkRoot "Include\$version\ucrt"),(Join-Path $sdkRoot "Include\$version\shared"),(Join-Path $sdkRoot "Include\$version\um"))
$compiler=Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe'
$linker=Join-Path $compilerRoot 'bin\Hostx64\x64\link.exe'
$object=Join-Path $output 'cache_state_test.obj'
$exe=Join-Path $output 'cache_state_test.exe'
$arguments=@('/nologo','/c','/TC','/W4','/WX','/MT','/O2')
foreach($include in $includes){$arguments+=('/I'+$include)}
$arguments+=('/Fo'+$object);$arguments+=(Join-Path $root 'tests\driver\transparent_source_test.c')
& $compiler @arguments
if($LASTEXITCODE){throw 'Shared driver-state test compilation failed.'}
& $linker /nologo /subsystem:console ('/OUT:'+$exe) $object ('/LIBPATH:'+(Join-Path $compilerRoot 'lib\x64')) ('/LIBPATH:'+(Join-Path $sdkRoot "Lib\$version\ucrt\x64")) ('/LIBPATH:'+(Join-Path $sdkRoot "Lib\$version\um\x64")) kernel32.lib bcrypt.lib
if($LASTEXITCODE){throw 'Shared driver-state test linking failed.'}
$result=& $exe
if($LASTEXITCODE){throw 'Shared driver-state tests failed.'}
$result | Set-Content -LiteralPath (Join-Path $output 'result.json') -Encoding utf8NoBOM
$result
