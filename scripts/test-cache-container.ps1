$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Run the owned SSD-container test with ordinary Windows administrator rights.'}
$workspace=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $workspace 'artifacts\runtime'
$run=[Guid]::NewGuid().ToString('N')
$root=Join-Path $runtime ('cache-control-'+$run)
$report=Join-Path $runtime ('cache-container-'+$run+'.json')
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;phase='starting';passed=$false;error=$null;root=$root;report=$report;hostDriverInstalled=$false;hostSecurityChanged=$false;sourceDataWritten=$false;globalAccelerationActive=$false}
$output=Join-Path $runtime 'cache-container-result.json'
function Save-Result{$result|ConvertTo-Json -Depth 14|Set-Content -LiteralPath $output -Encoding utf8NoBOM}
try{
    Save-Result
    & (Join-Path $workspace 'target\release\examples\container_probe.exe') $root $report *> (Join-Path $runtime ('cache-container-'+$run+'.log'))
    if(Test-Path -LiteralPath $report){$result.probe=Get-Content -LiteralPath $report -Raw|ConvertFrom-Json}
    if($LASTEXITCODE -or -not $result.probe.passed -or -not $result.probe.rootRemoved){throw 'Native container topology/cleanup test failed; inspect its log and report.'}
    $result.passed=$true;$result.phase='completed'
}catch{$result.phase='failed';$result.error=$_.Exception.Message}finally{
    $result.toolSha256=(Get-FileHash -LiteralPath (Join-Path $workspace 'target\release\examples\container_probe.exe')).Hash
    $result.sourceHashes=@('crates\storage-control\Cargo.toml','crates\storage-control\src\lib.rs','crates\storage-control\src\lab.rs','crates\storage-control\src\lab_windows.rs','crates\storage-control\src\windows.rs','crates\storage-control\src\journal.rs','crates\storage-control\src\format.rs','crates\storage-control\src\raw_volume.rs','crates\storage-control\src\session.rs','crates\storage-control\examples\container_probe.rs','scripts\test-cache-container.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $workspace $_)|Select-Object Path,Hash})
    Save-Result
}
if(-not $result.passed){throw $result.error}
