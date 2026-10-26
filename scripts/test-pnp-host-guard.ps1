$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $root 'artifacts\runtime'
$source=Join-Path $runtime ('superhdd-pnp-proof-host-guard-'+[Guid]::NewGuid().ToString('N'))
$reportPath=Join-Path $runtime 'pnp-probe-host-output.json'
if(Test-Path -LiteralPath $reportPath){throw 'Choose a fresh report path; existing evidence is not overwritten.'}
$probe=Join-Path $root 'target\release\examples\pnp_probe.exe'
$control=Join-Path $root 'target\release\examples\pnp_lab.exe'
$stdout=Join-Path $runtime 'pnp-host-guard-stdout.txt'
$stderr=Join-Path $runtime 'pnp-host-guard-stderr.txt'
$arguments='"'+$source+'" "'+$reportPath+'"'
$process=Start-Process -FilePath $probe -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if(-not $process.WaitForExit(20000)){$process.Kill();throw 'Host refusal timed out; no driver test result was inferred.'}
$process.Refresh()
$message=[IO.File]::ReadAllText($stderr)
$refused=$process.ExitCode -ne 0 -and $message -match 'Probe refused: run only inside an isolated disposable Windows VM'
$sourceAbsent=-not (Test-Path -LiteralPath $source)
$reportAbsent=-not (Test-Path -LiteralPath $reportPath)
$rawProcess=Start-Process -FilePath $probe -ArgumentList ('--raw '+$arguments) -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if(-not $rawProcess.WaitForExit(20000)){$rawProcess.Kill();throw 'RAW host refusal timed out.'}
$rawProcess.Refresh()
$rawRefused=$rawProcess.ExitCode -ne 0 -and [IO.File]::ReadAllText($stderr) -match 'Probe refused: run only inside an isolated disposable Windows VM'
$benchmark=Join-Path $root 'target\release\examples\raw_benchmark.exe'
$benchmarkProcess=Start-Process -FilePath $benchmark -ArgumentList 'stress missing-manifest.json missing-request.json forbidden-report.json' -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr
if(-not $benchmarkProcess.WaitForExit(20000)){$benchmarkProcess.Kill();throw 'Benchmark host refusal timed out.'}
$benchmarkProcess.Refresh()
$benchmarkRefused=$benchmarkProcess.ExitCode -ne 0 -and [IO.File]::ReadAllText($stderr) -match 'Benchmark refused: disposable Super HDD VM only'
$sourceAbsent=$sourceAbsent -and -not (Test-Path -LiteralPath $source)
$reportAbsent=$reportAbsent -and -not (Test-Path -LiteralPath $reportPath)
$controlHelp=& $control --help
if($LASTEXITCODE -ne 0){throw 'Control help failed.'}
$probeHelp=& $probe --help
if($LASTEXITCODE -ne 0){throw 'Probe help failed.'}
$result=[ordered]@{
    at=[DateTime]::UtcNow.ToString('o');passed=($refused -and $rawRefused -and $benchmarkRefused -and $sourceAbsent -and $reportAbsent)
    scope='Physical-host refusal and help commands only; no kernel or positive ordinary-file test.'
    hostRefused=$refused;fixtureNotCreated=$sourceAbsent;positiveProofReportNotCreated=$reportAbsent
    rawHostRefused=$rawRefused
    benchmarkStressHostRefused=$benchmarkRefused
    controlHelp=$controlHelp -join "`n";probeHelp=$probeHelp -join "`n";kernelRuntimeTested=$false
}
$result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $runtime 'pnp-host-guard-result.json') -Encoding utf8NoBOM
$result | ConvertTo-Json -Depth 4
if(-not $result.passed){throw 'Host guard failed; inspect evidence without treating it as a kernel test.'}
