$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$probe=Join-Path $root 'artifacts\iscsi-probe'
$out=Join-Path $root 'artifacts\loopback-runtime'
$null=New-Item -ItemType Directory -Path $out -Force
foreach($name in @('ISCSI.dll','DiskAccessLibrary.dll','transport-build.json')){Copy-Item -LiteralPath (Join-Path $probe $name) -Destination $out}
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /warnaserror+ /platform:x64 /target:exe ('/out:'+(Join-Path $out 'SuperHDDCacheWorker.exe')) ('/reference:'+(Join-Path $out 'ISCSI.dll')) ('/reference:'+(Join-Path $out 'DiskAccessLibrary.dll')) (Join-Path $root 'tools\loopback-runtime\Worker.cs') (Join-Path $root 'tools\loopback-runtime\PortLease.cs') (Join-Path $root 'tools\iscsi-cache-probe\KernelInitiatorPeer.cs')
if($LASTEXITCODE){throw 'Native runtime worker build failed.'}
. (Join-Path $PSScriptRoot 'enter-dev.ps1')
$previousTarget=$env:CARGO_TARGET_DIR;$previousFlags=$env:RUSTFLAGS
try{
    $env:CARGO_TARGET_DIR=Join-Path $root 'target\loopback-runtime'
    $env:RUSTFLAGS='-C target-feature=+crt-static'
    cargo build --release --locked --manifest-path (Join-Path $root 'tools\loopback-runtime\Cargo.toml')
    if($LASTEXITCODE){throw 'Native protected provider build failed.'}
    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release\superhdd_loopback_runtime.dll') -Destination $out
}finally{$env:CARGO_TARGET_DIR=$previousTarget;$env:RUSTFLAGS=$previousFlags}
[ordered]@{binaries=@('SuperHDDCacheWorker.exe','superhdd_loopback_runtime.dll','ISCSI.dll','DiskAccessLibrary.dll'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $out $_)|Select-Object Path,Hash});scope='Candidate worker build only; requires protected configuration and durable prepared journal; does not install a service or activate D'}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $out 'build.json') -Encoding utf8NoBOM
