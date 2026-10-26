$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$packages=Join-Path $root '.tools\iscsi-research'
$out=Join-Path $root 'artifacts\iscsi-probe'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$sourcePaths=@('crates\cache-core\src\lib.rs','tools\iscsi-cache-probe\src\lib.rs','tools\iscsi-cache-probe\src\device.rs','tools\iscsi-cache-probe\Probe.cs','tools\iscsi-cache-probe\Protocol.cs','tools\iscsi-cache-probe\CompleteSocketSend.cs','crates\storage-control\src\iscsi.rs','crates\storage-control\src\data_source.rs','crates\storage-control\examples\iscsi_native_probe.rs','crates\storage-control\examples\support\native_source.rs','scripts\build-private-iscsi-transport.ps1')
$builtSources=@($sourcePaths|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$builtSources+=Get-FileHash -LiteralPath (Join-Path $root 'tools\iscsi-cache-probe\KernelInitiatorPeer.cs')|Select-Object Path,Hash
$storageInputs=@(rg --files (Join-Path $root 'crates\storage-control') -g '*.rs' -g Cargo.toml)
$builtSources+=@($storageInputs|Where-Object {$_ -notin $builtSources.Path}|ForEach-Object {Get-FileHash -LiteralPath $_|Select-Object Path,Hash})
# The optional LGPL libraries are dynamically linked only by this private probe.
# They are not added to the desktop package or its runtime qualification manifest.
foreach($package in @(@{id='iscsi';version='1.5.6';dll='ISCSI.dll'},@{id='diskaccesslibrary';version='1.6.3';dll='DiskAccessLibrary.dll'})){
    $dll=Join-Path $packages ($package.id+'\lib\net40\'+$package.dll)
    if(-not(Test-Path -LiteralPath $dll)){
        New-Item -ItemType Directory -Path $packages -Force | Out-Null
        $archive=Join-Path $packages ($package.id+'.zip')
        Invoke-WebRequest ('https://api.nuget.org/v3-flatcontainer/{0}/{1}/{0}.{1}.nupkg' -f $package.id,$package.version) -OutFile $archive
        Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $packages $package.id) -Force
    }
    Copy-Item -LiteralPath $dll -Destination $out
}
& (Join-Path $PSScriptRoot 'build-private-iscsi-transport.ps1')
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /warnaserror+ /platform:x64 /target:exe ('/out:'+(Join-Path $out 'iscsi-probe.exe')) ('/reference:'+(Join-Path $out 'ISCSI.dll')) ('/reference:'+(Join-Path $out 'DiskAccessLibrary.dll')) (Join-Path $root 'tools\iscsi-cache-probe\Probe.cs') (Join-Path $root 'tools\iscsi-cache-probe\Protocol.cs') (Join-Path $root 'tools\iscsi-cache-probe\KernelInitiatorPeer.cs')
if($LASTEXITCODE){throw 'C# probe build failed.'}
. (Join-Path $PSScriptRoot 'enter-dev.ps1')
$priorTarget=$env:CARGO_TARGET_DIR
$priorFlags=$env:RUSTFLAGS
try{
    $env:CARGO_TARGET_DIR=Join-Path $root 'target\iscsi-probe'
    $env:RUSTFLAGS='-C target-feature=+crt-static'
    cargo build --release --locked --manifest-path (Join-Path $root 'tools\iscsi-cache-probe\Cargo.toml')
    if($LASTEXITCODE){throw 'Rust cache bridge build failed.'}
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\superhdd_iscsi_probe.dll') -Destination $out
    $env:CARGO_TARGET_DIR=Join-Path $root 'target\iscsi-native'
    cargo build --release --locked --manifest-path (Join-Path $root 'Cargo.toml') -p superhdd-storage-control --example iscsi_native_probe
    if($LASTEXITCODE){throw 'Native initiator controller build failed.'}
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\examples\iscsi_native_probe.exe') -Destination (Join-Path $out 'native-initiator.exe')
}finally{$env:CARGO_TARGET_DIR=$priorTarget;$env:RUSTFLAGS=$priorFlags}
foreach($source in $builtSources){if((Get-FileHash -LiteralPath $source.Path).Hash -ne $source.Hash){throw 'Source changed during build; refusing mixed build evidence.'}}
[ordered]@{at=[DateTime]::UtcNow.ToString('o');sources=$builtSources;transport=Get-Content -LiteralPath (Join-Path $out 'transport-build.json') -Raw|ConvertFrom-Json;binaries=@('iscsi-probe.exe','superhdd_iscsi_probe.dll','ISCSI.dll','DiskAccessLibrary.dll','native-initiator.exe'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $out $_)|Select-Object Path,Hash})}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $out 'build.json') -Encoding utf8NoBOM
Get-FileHash -LiteralPath (Join-Path $out 'iscsi-probe.exe'),(Join-Path $out 'superhdd_iscsi_probe.dll')
