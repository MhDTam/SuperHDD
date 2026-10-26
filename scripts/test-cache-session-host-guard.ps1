$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $workspace 'artifacts\runtime'
$run=[Guid]::NewGuid().ToString('N')
$fixture=Join-Path $runtime ('cache-session-host-refusal-'+$run)
$request=Join-Path $runtime ('cache-session-request-'+$run+'.json')
$log=Join-Path $runtime ('cache-session-refusal-'+$run+'.log')
$tool=Join-Path $workspace 'target\release\SuperHDDService.exe'
if(Get-CimInstance Win32_SystemDriver -Filter "Name LIKE '%SuperHDD%'"|Where-Object State -eq 'Running'){throw 'Refusal check requires no active host cache driver.'}
@{sourceVolume='D:';containerRoot=$fixture;budgetBytes=268435456;acknowledgedDisposableSource=$true}|ConvertTo-Json|Set-Content -LiteralPath $request -Encoding utf8NoBOM
& $tool --lab-cache-session $request *> $log
$code=$LASTEXITCODE
$message=Get-Content -LiteralPath $log -Raw
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');passed=($code -ne 0 -and $message.Contains('RAW lab driver unavailable') -and -not(Test-Path -LiteralPath $fixture));driverMissingSurfaced=$message.Contains('RAW lab driver unavailable');rootNotCreated=(-not(Test-Path -LiteralPath $fixture));exitCode=$code;serviceSha256=(Get-FileHash -LiteralPath $tool).Hash}
$labFixture=Join-Path $runtime ('managed-'+$run)
@{sourceVolume='D:';containerRoot=$labFixture;budgetBytes=268435456;acknowledgedDisposableSource=$true;isolatedLab=@{vmId='0150b344-482c-4758-b3e4-9f4bbf2ec015';runId=$run;sourceParentVolume='\\?\Volume{00000000-0000-0000-0000-000000000001}\';cacheParentVolume='\\?\Volume{00000000-0000-0000-0000-000000000002}\'}}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $request -Encoding utf8NoBOM
& $tool --lab-cache-session $request *> ($log+'.lab')
$labCode=$LASTEXITCODE
$labMessage=Get-Content -LiteralPath ($log+'.lab') -Raw
$result.isolatedLabHostRefused=($labCode -ne 0 -and $labMessage.Contains('refused outside the provisioned Super HDD VM') -and -not(Test-Path -LiteralPath $labFixture))
$result.passed=[bool]($result.passed -and $result.isolatedLabHostRefused)
$protectedRoot='C:\ProgramData\SuperHDDManagedLab'
$existed=Test-Path -LiteralPath $protectedRoot
& $tool --prepare-managed-lab $request *> ($log+'.scm')
$scmCode=$LASTEXITCODE
$scmMessage=Get-Content -LiteralPath ($log+'.scm') -Raw
$result.scmPrepareHostRefused=($scmCode -ne 0 -and $scmMessage.Contains('refused outside the provisioned Super HDD VM') -and (Test-Path -LiteralPath $protectedRoot) -eq $existed)
$result.passed=[bool]($result.passed -and $result.scmPrepareHostRefused)
$result.sourceHashes=@('services\superhdd-service\src\main.rs','services\superhdd-service\src\lab_control.rs','crates\platform\src\managed_cache.rs','crates\storage-control\src\session.rs','crates\storage-control\src\lab.rs','crates\storage-control\src\lab_windows.rs','scripts\test-cache-session-host-guard.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $workspace $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('services\superhdd-service\src\managed_host.rs','crates\storage-control\src\protected.rs','crates\storage-control\src\recovery.rs'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $workspace $_)|Select-Object Path,Hash})
$result|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $runtime 'cache-session-host-refusal.json') -Encoding utf8NoBOM
if(-not $result.passed){throw 'Native session host-refusal check failed; inspect its recorded log.'}
