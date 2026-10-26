$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
    throw 'Run the owned SSD-container lifecycle test with ordinary Windows administrator rights.'
}
$workspace=Split-Path -Parent $PSScriptRoot
$runtime=Join-Path $workspace 'artifacts\runtime'
$run=[Guid]::NewGuid().ToString('N')
$tool=Join-Path $workspace 'target\release\examples\container_lifecycle_probe.exe'
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;passed=$false;phase='starting';error=$null;hostDriverInstalled=$false;hostSecurityChanged=$false;sourceDataWritten=$false;globalAccelerationActive=$false;cycles=@()}
$output=Join-Path $runtime 'cache-lifecycle-result.json'
$child=$null
function Save-Result { $result|ConvertTo-Json -Depth 16|Set-Content -LiteralPath $output -Encoding utf8NoBOM }
try {
    Save-Result
    $result.priorFixtureCleanup=@()
    foreach($fixture in Get-ChildItem -LiteralPath $runtime -Directory -Filter 'cache-lifecycle-*'){
        $resolved=[IO.Path]::GetFullPath($fixture.FullName)
        if($fixture.Name -notmatch '^cache-lifecycle-[a-f0-9]{32}$' -or (Split-Path -Parent $resolved) -ne [IO.Path]::GetFullPath($runtime) -or ($fixture.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Unexpected prior fixture path; refusing cleanup.'}
        if(@(Get-ChildItem -LiteralPath $resolved -Force).Count -eq 0){Remove-Item -LiteralPath $resolved -ErrorAction Stop;continue}
        $priorReport=Join-Path $runtime ('cache-lifecycle-prior-recovery-'+[Guid]::NewGuid().ToString('N')+'.json')
        & $tool recover $resolved $priorReport *> ($priorReport+'.log')
        if($LASTEXITCODE){throw 'Previous owned fixture could not be recovered; preserving it.'}
        $result.priorFixtureCleanup+=@(Get-Content -LiteralPath $priorReport -Raw|ConvertFrom-Json)
    }
    foreach($mode in 'prepare','hold') {
        $id=[Guid]::NewGuid().ToString('N')
        $root=Join-Path $runtime ('cache-lifecycle-'+$id)
        $prepared=Join-Path $runtime ('cache-lifecycle-'+$id+'-prepared.json')
        $recovered=Join-Path $runtime ('cache-lifecycle-'+$id+'-recovered.json')
        $log=Join-Path $runtime ('cache-lifecycle-'+$id+'.log')
        $result.phase=$mode;Save-Result
        if($mode -eq 'hold') {
            $child=Start-Process -FilePath $tool -ArgumentList @($mode,('"'+$root+'"'),('"'+$prepared+'"')) -PassThru -WindowStyle Hidden -RedirectStandardOutput $log -RedirectStandardError ($log+'.stderr')
            $deadline=[DateTime]::UtcNow.AddSeconds(25)
            while(-not(Test-Path -LiteralPath $prepared) -and -not $child.HasExited -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100;$child.Refresh()}
            if(-not(Test-Path -LiteralPath $prepared)){throw 'Owned crash fixture did not reach its durable checkpoint.'}
            $checkpoint=Get-Content -LiteralPath $prepared -Raw|ConvertFrom-Json
            if(-not $checkpoint.passed -or $child.HasExited){throw 'Owned crash fixture failed before termination.'}
            Stop-Process -Id $child.Id -ErrorAction Stop
            if(-not $child.WaitForExit(10000)){throw 'Owned crash fixture did not exit.'}
            $child.Dispose();$child=$null
        }else {
            & $tool $mode $root $prepared *> $log
            if($LASTEXITCODE){throw 'Owned SSD prepare failed; inspect the cycle log.'}
            $checkpoint=Get-Content -LiteralPath $prepared -Raw|ConvertFrom-Json
        }
        $result.phase='recovering-'+$mode;Save-Result
        & $tool recover $root $recovered *> ($log+'.recovery')
        if($LASTEXITCODE){throw 'Owned SSD recovery failed; preserving its files for inspection.'}
        $recovery=Get-Content -LiteralPath $recovered -Raw|ConvertFrom-Json
        if(-not $checkpoint.passed -or -not $recovery.passed -or -not $recovery.removed -or (Test-Path -LiteralPath $root) -or
            [Convert]::ToBase64String([byte[]]$checkpoint.instance) -ne [Convert]::ToBase64String([byte[]]$recovery.instance) -or
            $checkpoint.ownedPath -ne $recovery.ownedPath){throw 'Ownership/recovery/cleanup assertions failed.'}
        $result.cycles+=@{mode=$mode;processKilled=($mode -eq 'hold');prepared=$checkpoint;recovered=$recovery;rootRemoved=$true}
    }
    $result.passed=$true;$result.phase='completed'
}catch{$result.error=$_.Exception.Message;$result.phase='failed'}finally{
    if($child){if(-not $child.HasExited){Stop-Process -Id $child.Id -ErrorAction SilentlyContinue};$child.Dispose()}
    $result.toolSha256=(Get-FileHash -LiteralPath $tool).Hash
    $result.sourceHashes=@('crates\storage-control\Cargo.toml','crates\storage-control\src\lib.rs','crates\storage-control\src\lab.rs','crates\storage-control\src\lab_windows.rs','crates\storage-control\src\windows.rs','crates\storage-control\src\journal.rs','crates\storage-control\src\format.rs','crates\storage-control\src\raw_volume.rs','crates\storage-control\src\session.rs','crates\storage-control\examples\container_lifecycle_probe.rs','scripts\test-cache-lifecycle.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $workspace $_)|Select-Object Path,Hash})
    Save-Result
}
if(-not $result.passed){throw $result.error}
