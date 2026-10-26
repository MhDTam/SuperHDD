# Read-only diagnostics for the exact protected disposable owner.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$manifest=Get-Content -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json' -Raw|ConvertFrom-Json
if(-not $manifest.lab -or $manifest.run -notmatch '^[a-f0-9]{32}$' -or $manifest.source.bytes -ne 128MB){throw 'Disposable protected owner required.'}
$control=Join-Path 'C:\ProgramData\SuperHDDLoopback' $manifest.run
$state=Get-Content -LiteralPath (Join-Path $control 'service-state.json') -Raw|ConvertFrom-Json
$ready=Get-Content -LiteralPath (Join-Path $control 'ready.json') -Raw|ConvertFrom-Json
$fileState=try{$test=[IO.File]::Open('S:\fixture.bin',[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite);$test.Dispose();'readable'}catch{$_.Exception.ToString()}
[ordered]@{run=$manifest.run;state=$state;ready=$ready;fileState=$fileState;service=(Get-CimInstance Win32_Service -Filter "Name='SuperHDDLoopbackLab'"|Select-Object Name,State,ProcessId);sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq ('iqn.2026-10.local.superhdd:cache-'+$manifest.run)|Select-Object SessionIdentifier,TargetNodeAddress);disks=@(Get-Disk|Where-Object {$_.Number -in @($manifest.source.number,$state.projectedDisk)}|Select-Object Number,IsOffline,IsReadOnly,OperationalStatus);worker=(Get-Process -Id $ready.pid -ErrorAction SilentlyContinue|Select-Object Id,Path)}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $root 'artifacts\loopback-scm-diagnostic.json') -Encoding utf8NoBOM
