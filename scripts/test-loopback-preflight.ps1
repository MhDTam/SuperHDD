# Read-only service integration check: no offline/format/cache allocation command.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$exe=Join-Path $root 'target\loopback-service\release\SuperHDDService.exe'
$configured=Get-Content -LiteralPath (Join-Path $root 'artifacts\cache-25gb-setup.json') -Raw|ConvertFrom-Json
$command=[ordered]@{operation='start';request=$configured.request}|ConvertTo-Json -Compress
$hex=[Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes($command))
$result=[ordered]@{passed=$false;readOnly=$true;active=$false;error=$null;binary=Get-FileHash -LiteralPath $exe|Select-Object Path,Hash}
$before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$source=Get-Partition -DriveLetter D|Get-Disk
$cache=Get-Partition -DriveLetter M|Get-Disk
try{
    $json=& $exe --loopback-preflight $hex 2> (Join-Path $root 'artifacts\loopback-preflight-error.log')
    if($LASTEXITCODE){throw 'Native service preflight rejected the current hardware context.'}
    $result.preflight=$json|ConvertFrom-Json
    $result.passed=($result.preflight.preflightPassed -and $result.preflight.readOnly -and -not $result.preflight.active)
}catch{$result.error=$_.Exception.Message}finally{
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootUnchanged=($before.machineSha256 -eq $after.machineSha256 -and $before.secureBootEnabled -eq $after.secureBootEnabled -and $before.codeIntegrityOptions -eq $after.codeIntegrityOptions)
    $result.parentsUnchanged=((Get-Partition -DriveLetter D|Get-Disk).UniqueId -eq $source.UniqueId -and (Get-Partition -DriveLetter M|Get-Disk).UniqueId -eq $cache.UniqueId)
    $result.passed=[bool]($result.passed -and $result.bootUnchanged -and $result.parentsUnchanged)
    $result|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $root 'artifacts\loopback-preflight.json') -Encoding utf8NoBOM
}
if(-not $result.passed){throw 'Read-only native service preflight failed.'}
