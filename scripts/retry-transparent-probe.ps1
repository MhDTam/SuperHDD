param([Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Run)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot;$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$record=Join-Path $root 'artifacts\transparent-session.json';$r=Get-Content $record -Raw|ConvertFrom-Json -AsHashtable
if($r.runId -ne $Run -or -not $r.install.pagefileConfigured){throw 'Require the exact prepared disposable run'}
$vm=Get-VM -Id $r.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count -or $vm.State -ne 'Running'){throw 'Exact running isolated VM required'}
$hostRun=Join-Path $lab ('transparent-'+$Run);$guestRun='C:\SuperHDDLab\transparent-'+$Run
$lock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$session=$null
try{
    Copy-Item -LiteralPath $record -Destination (Join-Path $hostRun ('prior-proof-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff')+'.json'))
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    $session=New-PSSession -VMId $vm.Id -Credential $credential
    $identity=Invoke-Command -Session $session -ArgumentList $vm.Id.ToString(),$guestRun -ScriptBlock {
        param($id,$dir)
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $id){throw 'Wrong VM'}
        $request=Get-Content (Join-Path $dir 'request.json') -Raw|ConvertFrom-Json
        if((Get-Volume -DriveLetter S).Path -ne $request.isolatedLab.sourceParentVolume -or (Get-Volume -DriveLetter K).Path -ne $request.isolatedLab.cacheParentVolume){throw 'Prepared source/cache identity changed'}
        if(-not(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys')){throw 'Source page file is not active'}
        if(Get-Process -Name probe -ErrorAction SilentlyContinue){throw 'A probe process is already running'}
        $true
    }
    if(-not $identity){throw 'VM identity check failed'}
    $probe=Join-Path $root 'target\transparent-kernel\release\examples\transparent_session_probe.exe'
    $r.probeSha256=(Get-FileHash $probe).Hash
    Copy-Item -ToSession $session -LiteralPath $probe -Destination (Join-Path $guestRun 'probe.exe')
    $r.probe=Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {
        param($dir)
        $ErrorActionPreference='Stop'
        $p=Start-Process -FilePath (Join-Path $dir 'probe.exe') -ArgumentList ('"'+(Join-Path $dir 'request.json')+'" "'+(Join-Path $dir 'proof.json')+'"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $dir 'probe.log') -RedirectStandardError (Join-Path $dir 'probe-error.log')
        if(-not $p.WaitForExit(180000)){$p.Kill();throw 'Owned probe timed out'}
        Get-Content (Join-Path $dir 'proof.json') -Raw|ConvertFrom-Json
    }
    $r.phase=if($r.probe.passed){'probe-passed-cleanup-pending'}else{'probe-failed'}
    Copy-Item -FromSession $session -LiteralPath $guestRun -Destination (Join-Path $hostRun ('attempt-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff'))) -Recurse
}finally{if($session){Remove-PSSession $session};$lock.Dispose();$r|ConvertTo-Json -Depth 15|Set-Content $record -Encoding utf8NoBOM}
