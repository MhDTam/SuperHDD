$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$prior=Join-Path $root 'artifacts\runtime\kernel-lab\direct-session.json'
if(Test-Path -LiteralPath $prior){
    $p=Get-Content -LiteralPath $prior -Raw|ConvertFrom-Json
    if($p.runId -in 'cbb2abf8c33b44f7a40672725f77f019','d37725786d8541a3ae14ebd5ea8160cd' -and $p.cleanupError -in 'V5 driver unload failed.','V5 driver remains registered.' -and (Test-Path -LiteralPath $p.parents[0].path)){
        & (Join-Path $PSScriptRoot 'cleanup-direct-prior.ps1') -RunId $p.runId
    }
}
& (Join-Path $PSScriptRoot 'test-direct-session.ps1')
