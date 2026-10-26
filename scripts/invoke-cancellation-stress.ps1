param([switch]$Verifier,[switch]$UseActiveVerifier,[switch]$ForcePending)
$ErrorActionPreference='Stop'
if($UseActiveVerifier -and -not $Verifier){throw 'An active Verifier run must request Verifier evidence.'}
if($ForcePending -and -not $Verifier){throw 'Forced pending I/O requires guest-only Driver Verifier.'}
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$runLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$session=$null
$runId=[Guid]::NewGuid().ToString('N')
$guestDirectory='C:\SuperHDDLab\stress-run-'+$runId
$resultPath=Join-Path $lab $(if($ForcePending){'verifier-pending-stress.json'}elseif($Verifier){'verifier-stress.json'}else{'cancellation-stress.json'})
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$runId;phase='initializing';passed=$false;error=$null;hostSecurityChanged=$false;guestDirectory=$guestDirectory;verifierRequested=[bool]$Verifier;forcePendingRequested=[bool]$ForcePending}
function Save-Result {$result|ConvertTo-Json -Depth 24|Set-Content -LiteralPath $resultPath -Encoding utf8NoBOM}
try{
    if(Test-Path -LiteralPath $resultPath){
        $prior=Get-Content -LiteralPath $resultPath -Raw|ConvertFrom-Json
        if($prior.phase -ne 'completed' -or -not $prior.fixtureRemoved){throw 'Previous stress run needs inspection before another run.'}
        Move-Item -LiteralPath $resultPath -Destination (Join-Path $lab (([IO.Path]::GetFileNameWithoutExtension($resultPath))+'-prior-'+$prior.runId+'.json'))
    }
    $configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $configuration.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Disposable VM isolation identity failed.'}
    $result.vmId=$vm.Id.ToString();Save-Result
    if($vm.State -eq 'Off'){Start-VM -VM $vm|Out-Null}
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    for($attempt=0;$attempt -lt 40;$attempt++){
        try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 3}
    }
    if(-not $session){throw 'Disposable guest did not become ready.'}
    if($Verifier -and -not $UseActiveVerifier){
        $result.phase='verifier-one-boot-configuration';Save-Result
        $result.verifierConfiguration=Invoke-Command -Session $session -ArgumentList ([bool]$ForcePending) -ScriptBlock {
            param($Pending)
            if($env:COMPUTERNAME -ne 'SHDD-LAB'){throw 'Wrong Verifier guest.'}
            $driver=Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'"
            if($driver.PathName -ne '\??\C:\SuperHDDLab\driver.sys' -and $driver.PathName -ne 'C:\SuperHDDLab\driver.sys'){throw 'Wrong disposable driver image.'}
            & C:\SuperHDDLab\raw_lab.exe disable *> C:\SuperHDDLab\verifier-disable.log
            if($LASTEXITCODE){throw 'Cannot disable lab session before Verifier reboot.'}
            $settings=if($Pending){& verifier.exe /flags 0x20bbb /driver driver.sys 2>&1|Out-String}else{& verifier.exe /standard /driver driver.sys 2>&1|Out-String}
            $settingsExit=$LASTEXITCODE
            $boot=& verifier.exe /bootmode oneboot 2>&1|Out-String
            $bootExit=$LASTEXITCODE
            $query=& verifier.exe /querysettings 2>&1|Out-String
            # Configuration commands can return a nonzero reboot-needed code.
            # Require the exact target, complete standard mask and one-boot
            # read-back instead of inferring success from their exit code.
            $flags=if($query -match 'Verifier Flags:\s*0x([a-fA-F0-9]+)'){[Convert]::ToUInt32($Matches[1],16)}else{0}
            if(($flags -band 0x209bb) -ne 0x209bb -or ($Pending -and ($flags -band 0x200) -ne 0x200) -or $query -notmatch '(?is)Verified Drivers:\s*driver\.sys\s*$' -or $query -notmatch '(?is)Boot Mode:\s*One\s*Boot'){throw ('Guest one-boot Verifier read-back failed: '+$query)}
            [pscustomobject]@{settings=$settings;settingsExit=$settingsExit;bootMode=$boot;bootExit=$bootExit;query=$query;beforeBoot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
            & shutdown.exe /r /t 5|Out-Null
        }
        Remove-PSSession $session;$session=$null
        Start-Sleep -Seconds 10
        for($attempt=0;$attempt -lt 50;$attempt++){
            $candidate=$null
            try{
                $candidate=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop
                $boot=Invoke-Command -Session $candidate -ScriptBlock {(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
                if($boot -ne $result.verifierConfiguration.beforeBoot){$session=$candidate;break}
            }catch{}
            if($candidate){Remove-PSSession $candidate -ErrorAction SilentlyContinue}
            Start-Sleep -Seconds 3
        }
        if(-not $session){throw 'Guest did not return from its one-boot Verifier restart.'}
    }
    $result.phase='restoring-owned-volumes';Save-Result
    $result.volumes=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()
    $binary=Join-Path $root 'target\release\examples\raw_benchmark.exe'
    Copy-Item -ToSession $session -LiteralPath $binary -Destination C:\SuperHDDLab\raw_benchmark.exe
    $result.toolSha256=(Get-FileHash -LiteralPath $binary).Hash
    $result.sourceHashes=@('crates\platform\examples\raw_benchmark.rs','crates\platform\examples\raw_stress\mod.rs','scripts\invoke-cancellation-stress.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
    $result.driver=Invoke-Command -Session $session -ScriptBlock {
        [pscustomobject]@{sha256=(Get-FileHash C:\SuperHDDLab\driver.sys).Hash;signature=(Get-AuthenticodeSignature C:\SuperHDDLab\driver.sys).Status.ToString()}
    }
    if($result.driver.signature -ne 'Valid'){throw 'Guest driver signature is not valid.'}
    if($Verifier){
        $result.verifierActive=Invoke-Command -Session $session -ArgumentList ([bool]$ForcePending) -ScriptBlock {
            param($Pending)
            $query=& verifier.exe /query 2>&1|Out-String
            $flags=if($query -match 'Verifier Flags:\s*0x([a-fA-F0-9]+)'){[Convert]::ToUInt32($Matches[1],16)}else{0}
            if($query -notmatch 'driver.sys' -or ($flags -band 0x209bb) -ne 0x209bb -or ($Pending -and ($flags -band 0x200) -ne 0x200)){throw 'Verifier is not active with the requested checks on the loaded disposable driver.'}
            [pscustomobject]@{query=$query;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
        }
    }
    $result.phase='creating-owned-fixture';Save-Result
    $result.preparation=Invoke-Command -Session $session -ArgumentList $guestDirectory,$runId -ScriptBlock {
        param($Directory,$RunId)
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or $Directory -notmatch '^C:\\SuperHDDLab\\stress-run-[a-f0-9]{32}$'){throw 'Wrong guest/run directory.'}
        New-Item -ItemType Directory -Path $Directory -ErrorAction Stop|Out-Null
        $fixture='R:\superhdd-kernel-bench-'+$RunId
        & C:\SuperHDDLab\raw_lab.exe disable *> (Join-Path $Directory 'disable.log')
        if($LASTEXITCODE){throw 'Cannot release the previous lab session.'}
        & C:\SuperHDDLab\raw_benchmark.exe prepare C:\SuperHDDLab\request.json $fixture (Join-Path $Directory 'manifest.json') 1024 *> (Join-Path $Directory 'prepare.log')
        if($LASTEXITCODE){throw 'Owned stress fixture preparation failed; inspect prepare.log.'}
        [pscustomobject]@{fixture=$fixture;bytes=64MB}
    }
    $result.phase='overlapped-reads-and-controls';Save-Result
    $result.stress=Invoke-Command -Session $session -ArgumentList $guestDirectory -ScriptBlock {
        param($Directory)
        $arguments='stress "'+(Join-Path $Directory 'manifest.json')+'" C:\SuperHDDLab\request.json "'+(Join-Path $Directory 'stress.json')+'"'
        $process=Start-Process -FilePath C:\SuperHDDLab\raw_benchmark.exe -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Directory 'stress-stdout.log') -RedirectStandardError (Join-Path $Directory 'stress-stderr.log')
        if(-not $process.WaitForExit(180000)){throw 'Stress tool exceeded three minutes; preserve guest and request ownership for inspection.'}
        $process.Refresh()
        if(-not(Test-Path -LiteralPath (Join-Path $Directory 'stress.json'))){throw ('Stress tool produced no report, exit '+$process.ExitCode)}
        Get-Content -LiteralPath (Join-Path $Directory 'stress.json') -Raw|ConvertFrom-Json
        if($process.ExitCode){throw ('Stress tool failed, exit '+$process.ExitCode)}
    }
    if(-not $result.stress.passed){throw 'Stress assertions failed.'}
    if($Verifier){
        $result.verifierAfter=Invoke-Command -Session $session -ScriptBlock {
            $query=& verifier.exe /query 2>&1|Out-String
            if($LASTEXITCODE -or $query -notmatch 'driver.sys'){throw 'Verifier driver target disappeared.'}
            [pscustomobject]@{query=$query;boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')}
        }
        if($result.verifierActive.boot -ne $result.verifierAfter.boot){throw 'Guest rebooted during the Verifier test.'}
    }
    $result.passed=$true
}catch{$result.error=$_.Exception.Message;$result.errorDetail=$_.ScriptStackTrace}finally{
    if($session){
        try{
            $result.cleanup=Invoke-Command -Session $session -ArgumentList $guestDirectory -ScriptBlock {
                param($Directory)
                $manifest=Join-Path $Directory 'manifest.json'
                if(Test-Path -LiteralPath $manifest){
                    & C:\SuperHDDLab\raw_benchmark.exe cleanup $manifest *> (Join-Path $Directory 'cleanup.log')
                    if($LASTEXITCODE){throw 'Owned stress fixture cleanup failed.'}
                    $identity=Get-Content -LiteralPath $manifest -Raw|ConvertFrom-Json
                    [pscustomobject]@{fixtureRemoved=(-not(Test-Path -LiteralPath $identity.root));cacheDetached=((& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json).cache.attached -eq $false)}
                }
            }
            $result.fixtureRemoved=[bool]$result.cleanup.fixtureRemoved
            $guestEvidenceExists=Invoke-Command -Session $session -ArgumentList $guestDirectory -ScriptBlock {param($Directory) Test-Path -LiteralPath $Directory}
            if($guestEvidenceExists){
                $destination=Join-Path $lab ('stress-run-'+$runId)
                Copy-Item -FromSession $session -LiteralPath $guestDirectory -Destination $destination -Recurse
                $result.evidenceDirectory=$destination
            }
        }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
        if($Verifier){
            try{
                $result.verifierReset=Invoke-Command -Session $session -ScriptBlock {
                    $text=& verifier.exe /reset 2>&1|Out-String
                    $resetExit=$LASTEXITCODE
                    $query=& verifier.exe /querysettings 2>&1|Out-String
                    if($query -match '(?is)Verified Drivers:\s*driver\.sys' -or $query -match 'Verifier Flags:\s*0x0*[1-9a-fA-F]'){throw ('Guest Verifier reset read-back failed: '+$query)}
                    [pscustomobject]@{resetForNextBoot=$true;resetExit=$resetExit;output=$text;querySettings=$query}
                }
            }catch{$result.verifierResetError=$_.Exception.Message;$result.passed=$false}
        }
        Remove-PSSession $session -ErrorAction SilentlyContinue
    }
    $result.passed=[bool]($result.passed -and $result.fixtureRemoved -and $result.cleanup.cacheDetached)
    $result.phase=if($result.passed){'completed'}else{'failed'}
    Save-Result
    $runLock.Dispose()
}
if(-not $result.passed){throw $result.error}
