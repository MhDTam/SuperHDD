param([ValidateSet('Status','Copy','Sign','PrepareVolumes','Probe','ResumeProbe')][string]$Action='Status',[switch]$SkipProbe)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$benchmarkLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw | ConvertFrom-Json
if(-not $configuration.vmId){throw 'No prepared lab VM identity.'}
$vm=Get-VM -Id $configuration.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Lab isolation identity check failed.'}
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');action=$Action;vmId=$vm.Id.ToString();state=$vm.State.ToString();hostSecurityChanged=$false;error=$null}
$session=$null
function Save-Result {$result|ConvertTo-Json -Depth 14|Set-Content -LiteralPath (Join-Path $lab ('action-'+$Action.ToLowerInvariant()+'.json')) -Encoding utf8NoBOM}
function Wait-GuestRestart([PSCredential]$Login){
    Start-Sleep -Seconds 15
    for($attempt=0;$attempt -lt 40;$attempt++){
        $connection=$null
        try{
            $connection=New-PSSession -VMId $vm.Id -Credential $Login -ErrorAction Stop
            $status=Invoke-Command -Session $connection -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'Status',$vm.Id.ToString()
            if($status.uptimeSeconds -lt 240){return $connection}
        }catch{}
        if($connection){Remove-PSSession -Session $connection -ErrorAction SilentlyContinue}
        Start-Sleep -Seconds 5
    }
    throw 'Guest did not become ready after its normal restart.'
}
try{
    if($vm.State -ne 'Running'){throw 'Lab VM is not running.'}
    if($Action -eq 'Sign'){
        $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
        $session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop
        $loaded=Invoke-Command -Session $session -ScriptBlock {(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'").State -eq 'Running'}
        if($loaded){$result.unload=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'Unload',$vm.Id.ToString()}
        Remove-PSSession -Session $session;$session=$null
        if($result.unload.rebootRequired){
            $result.phase='restarting-guest-to-release-old-image';Save-Result
            $session=Wait-GuestRestart $credential
            $stillLoaded=Invoke-Command -Session $session -ScriptBlock {(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'").State -eq 'Running'}
            if($stillLoaded){throw 'Old lab image is still running; refusing replacement.'}
            Remove-PSSession -Session $session;$session=$null
        }
    }
    if($Action -in 'Copy','Sign','Probe','ResumeProbe'){
        $files=@{
            'artifacts\driver-raw-lab\SuperHDDCacheDriver.sys'='driver.sys'
            'target\release\examples\raw_lab.exe'='raw_lab.exe'
            'target\release\examples\pnp_probe.exe'='pnp_probe.exe'
        }
        if($Action -in 'Probe','ResumeProbe'){$files.Remove('artifacts\driver-raw-lab\SuperHDDCacheDriver.sys')}
        foreach($file in $files.GetEnumerator()){
            Copy-VMFile -VM $vm -SourcePath (Join-Path $root $file.Key) -DestinationPath ('C:\SuperHDDLab\'+$file.Value) -CreateFullPath -FileSource Host -Force
        }
        $result.copiedHashes=@($files.GetEnumerator()|ForEach-Object{@{name=$_.Value;sha256=(Get-FileHash -LiteralPath (Join-Path $root $_.Key)).Hash}})
        $signTool='C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe'
        if(-not(Test-Path -LiteralPath $signTool)){$signTool='C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\signtool.exe'}
        if((Get-AuthenticodeSignature -LiteralPath $signTool).Status -ne 'Valid'){throw 'SDK signing tool signature is not valid.'}
        Copy-VMFile -VM $vm -SourcePath $signTool -DestinationPath 'C:\SuperHDDLab\signtool.exe' -CreateFullPath -FileSource Host -Force
        $result.copied=$true
    }
    if($Action -ne 'Copy'){
        $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
        $session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop
        $guestAction=if($Action -eq 'ResumeProbe'){'ResumeVolumes'}else{$Action}
        $guest=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList $guestAction,$vm.Id.ToString()
        $result.guest=$guest
        Save-Result
        if($Action -eq 'ResumeProbe'){
            $result.volumes=$guest;$result.phase='kernel-probe';Save-Result
            $result.probe=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'Probe',$vm.Id.ToString()
            Copy-Item -FromSession $session -LiteralPath C:\SuperHDDLab\probe.json -Destination (Join-Path $lab 'probe.json')
            $result.phase='completed-lab-probe';Save-Result
        }
        # One ordinary elevation covers the already-authorized isolated lab
        # workflow. No host security setting or host driver is changed.
        if($Action -eq 'Sign' -and $guest.signed -and -not $SkipProbe){
            if($guest.guestRebootScheduled){
                Remove-PSSession -Session $session;$session=$null
                $result.phase='waiting-for-guest-reboot';Save-Result
                $session=Wait-GuestRestart $credential
            }
            $result.phase='preparing-disposable-volumes';Save-Result
            $existing=Invoke-Command -Session $session -ScriptBlock {Test-Path -LiteralPath C:\SuperHDDLab\source.vhdx}
            $prepareAction=if($existing){'ResumeVolumes'}else{'PrepareVolumes'}
            $result.volumes=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList $prepareAction,$vm.Id.ToString()
            $result.phase='kernel-probe';Save-Result
            $result.probe=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'Probe',$vm.Id.ToString()
            Copy-Item -FromSession $session -LiteralPath C:\SuperHDDLab\probe.json -Destination (Join-Path $lab 'probe.json')
            $result.phase='completed-lab-probe';Save-Result
        }
        if($Action -eq 'Probe'){
            Copy-Item -FromSession $session -LiteralPath 'C:\SuperHDDLab\probe.json' -Destination (Join-Path $lab 'probe.json')
        }
    }
}catch{$result.error=$_.Exception.Message;$result.errorDetail=$_.ScriptStackTrace}finally{
    if($session){
        try{
            $logDirectory=Join-Path $lab 'guest-logs';New-Item -ItemType Directory -Path $logDirectory -Force|Out-Null
            Copy-Item -FromSession $session -Path 'C:\SuperHDDLab\*.log','C:\SuperHDDLab\sign-stage.txt' -Destination $logDirectory -ErrorAction SilentlyContinue
            Copy-Item -FromSession $session -LiteralPath C:\SuperHDDLab\probe.json -Destination (Join-Path $lab 'probe.json') -ErrorAction SilentlyContinue
            Copy-Item -FromSession $session -LiteralPath C:\SuperHDDLab\driver.sys -Destination (Join-Path $lab 'signed-driver.sys') -ErrorAction SilentlyContinue
        }catch{}
    }
    if($session){Remove-PSSession -Session $session -ErrorAction SilentlyContinue}
    if($result.phase -eq 'completed-lab-probe' -and -not $result.error -and (Test-Path -LiteralPath (Join-Path $lab 'probe.json'))){
        $result.probeArtifactSha256=(Get-FileHash -LiteralPath (Join-Path $lab 'probe.json')).Hash
    }
    Save-Result
    if($benchmarkLock){$benchmarkLock.Dispose()}
}
