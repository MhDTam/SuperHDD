# One elevated operation: finish only the recorded disposed first attempt,
# then run the expanded VM workload using new host HDD/SSD fixture files.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$record=Join-Path $root 'artifacts\paging-session.json'
$previous=Get-Content $record -Raw|ConvertFrom-Json
if($previous.runId -eq '8775edb357c84bba92a189f4bf2ac491'){
    if(-not $previous.hostParentsRemoved -or -not $previous.cleanup.filterRemoved -or -not $previous.cleanup.pagefileRestored){throw 'Initial run cleanup prerequisites are incomplete'}
    $archive=Join-Path $root 'artifacts\runtime\kernel-lab\paging-initial-8775edb357c84bba92a189f4bf2ac491.json'
    if(-not(Test-Path $archive)){Copy-Item -LiteralPath $record -Destination $archive}
    & (Join-Path $PSScriptRoot 'finish-paging-session.ps1') -Run $previous.runId -ShutdownOnly
}
& (Join-Path $PSScriptRoot 'test-paging-session.ps1') -HostHddFiles
