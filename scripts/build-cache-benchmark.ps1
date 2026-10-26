$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$output=Join-Path $root 'artifacts\cache-benchmark\bin'
[void][IO.Directory]::CreateDirectory($output)
$vs=& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$compiler=(Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory|Sort-Object Name -Descending|Select-Object -First 1).FullName
$sdk='C:\Program Files (x86)\Windows Kits\10';$version='10.0.26100.0'
$env:INCLUDE=(@((Join-Path $compiler 'include'),(Join-Path $sdk "Include\$version\ucrt"),(Join-Path $sdk "Include\$version\shared"),(Join-Path $sdk "Include\$version\um")) -join ';')
$env:LIB=(@((Join-Path $compiler 'lib\x64'),(Join-Path $sdk "Lib\$version\ucrt\x64"),(Join-Path $sdk "Lib\$version\um\x64")) -join ';')
$cl=Join-Path $compiler 'bin\Hostx64\x64\cl.exe'
foreach($variant in @('worker','probe','pool-test')){
 $source=switch($variant){worker{'worker.cpp'}probe{'cache-benchmark.cpp'}default{'pool-test.cpp'}}
 $extra=if($variant -eq 'worker'){@('/DSUPERHDD_STORAGE_BENCHMARK',('/I'+(Join-Path $root '.tools\dokan-sdk\2.3.1')))}else{@()}
 $libs=@(if($variant -eq 'worker'){'advapi32.lib';'bcrypt.lib'}elseif($variant -eq 'pool-test'){'bcrypt.lib'})
 & $cl /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX /DUNICODE /D_UNICODE @extra ('/Fo'+(Join-Path $output ($variant+'.obj'))) (Join-Path $root ('tools\loading-cache\'+$source)) /link ('/OUT:'+(Join-Path $output ($variant+'.exe'))) @libs
 if($LASTEXITCODE){throw ('Benchmark build failed: '+$variant)}
}
$testRoot=Join-Path $output ('tests-'+[Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
& (Join-Path $output 'pool-test.exe') $testRoot
if($LASTEXITCODE){throw 'Production-default pool regression failed'}
$sources=@{};foreach($name in @('worker.cpp','pool-v2.h','pool.h','admission.h','benchmark-io.h','cache-benchmark.cpp')){$sources[$name]=(Get-FileHash -LiteralPath (Join-Path $root ('tools\loading-cache\'+$name))).Hash}
[ordered]@{schemaVersion=1;scope='Storage-isolated build of the same worker and pool source; normal production workers are not replaced';workerSha256=(Get-FileHash (Join-Path $output worker.exe)).Hash;probeSha256=(Get-FileHash (Join-Path $output probe.exe)).Hash;compiler='MSVC /std:c++17 /O2 /MT';definition='SUPERHDD_STORAGE_BENCHMARK';sources=$sources}|ConvertTo-Json -Depth 5|Set-Content -Encoding UTF8 (Join-Path $output manifest.json)
