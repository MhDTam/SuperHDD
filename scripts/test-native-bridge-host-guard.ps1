$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $workspace 'artifacts\runtime'
$run=[Guid]::NewGuid().ToString('N')
$report=Join-Path $runtime ('native-bridge-host-refusal-'+$run+'.json')
$log=Join-Path $runtime ('native-bridge-host-refusal-'+$run+'.log')
$tool=Join-Path $workspace 'target\release\examples\native_bridge_probe.exe'
& $tool ('R:\superhdd-native-bridge-'+$run) $report *> $log
$code=$LASTEXITCODE
$message=Get-Content -LiteralPath $log -Raw
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');passed=($code -ne 0 -and $message.Contains('Native bridge probe refused: disposable Super HDD VM only') -and -not(Test-Path -LiteralPath $report));hostRefused=$message.Contains('Native bridge probe refused: disposable Super HDD VM only');reportNotCreated=(-not(Test-Path -LiteralPath $report));probeSha256=(Get-FileHash -LiteralPath $tool).Hash}
$result|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $runtime 'native-bridge-host-guard.json') -Encoding utf8NoBOM
if(-not $result.passed){throw 'The current native bridge probe did not refuse this physical host.'}
