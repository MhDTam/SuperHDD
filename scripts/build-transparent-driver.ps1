param([switch]$VmTest,[switch]$Analyze)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs=& $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if(-not $vs){throw 'Visual Studio C++ tools are required.'}
$compilerVersion=Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1
$compilerRoot=$compilerVersion.FullName
$kitRoot=Join-Path $root '.tools\wdk\c'
$sdkRoot='C:\Program Files (x86)\Windows Kits\10'
$version='10.0.26100.0'
$outputName=if($VmTest){'driver-transparent-vm'}else{'driver-transparent'}
if($Analyze){$outputName+='-analysis'}
$output=Join-Path $root ('artifacts\'+$outputName)
New-Item -ItemType Directory -Path $output -Force | Out-Null
$compiler=Join-Path $compilerRoot 'bin\Hostx64\x64\cl.exe'
$linker=Join-Path $compilerRoot 'bin\Hostx64\x64\link.exe'
$source=Join-Path $root 'drivers\superhdd-cache-driver\v7\volume_driver.c'
$object=Join-Path $output 'driver.obj'
$driver=Join-Path $output 'SuperHDDCacheDriver.sys'
$experimental=1
$includes=@((Join-Path $kitRoot "Include\$version\km"),(Join-Path $kitRoot "Include\$version\shared"),(Join-Path $sdkRoot "Include\$version\shared"),(Join-Path $sdkRoot "Include\$version\ucrt"),(Join-Path $compilerRoot 'include'))
$arguments=@('/nologo','/c','/TC','/kernel','/Zl','/GS','/O2','/W4','/WX','/D_AMD64_','/D_WIN64','/D_WIN32_WINNT=0x0A00','/DNTDDI_VERSION=0x0A000008',"/DSHDD_ENABLE_EXPERIMENTAL_ATTACH=$experimental")
$arguments+=@('/DSHDD_RAW_STORE=1',('/DSHDD_DIRECT_VM_BUILD='+[int][bool]$VmTest))
if($Analyze){$arguments+=@('/analyze','/analyze:external-','/external:W0')}
foreach($include in $includes){$arguments+=($(if($Analyze){'/external:I'}else{'/I'})+$include)}
$arguments+=('/Fo'+$object)
$arguments+=$source
& $compiler @arguments
if($LASTEXITCODE){throw 'Kernel source compilation failed.'}
& $linker /nologo /driver /subsystem:native /entry:GsDriverEntry /nodefaultlib /machine:x64 ('/OUT:'+$driver) $object ('/LIBPATH:'+(Join-Path $kitRoot "Lib\$version\km\x64")) ntoskrnl.lib hal.lib wdmsec.lib bufferoverflowfastfailk.lib
if($LASTEXITCODE){throw 'Kernel linking failed.'}
Get-FileHash -LiteralPath $driver | Format-List
Write-Output 'Compiled only. Unsigned, uninstalled, and not runtime-qualified. Do not load on user data.'
