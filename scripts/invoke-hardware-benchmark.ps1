param([ValidateSet('Run','Resume')][string]$Action='Run',[ValidateSet(1024,8192)][int]$FixtureBlocks=8192)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$stateFile=Join-Path $lab 'hardware-benchmark.json'
$runLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
$vm=Get-VM -Id $configuration.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or $vm.State -ne 'Running' -or @($vm|Get-VMNetworkAdapter).Count){throw 'Disposable VM isolation mismatch.'}
$session=$null
function Save-State {$state|ConvertTo-Json -Depth 18|Set-Content -LiteralPath $stateFile -Encoding utf8NoBOM}
function Snapshot([int]$Number){
    $row=@(Get-CimInstance Win32_PerfRawData_PerfDisk_PhysicalDisk|Where-Object {$_.Name -match ('^'+$Number+'(?: |$)')})
    if($row.Count -ne 1){throw "Physical counter identity is ambiguous for disk $Number"}
    [ordered]@{name=$row[0].Name;readOperations=[decimal]$row[0].DiskReadsPersec;readBytes=[decimal]$row[0].DiskReadBytesPersec;timestamp=[decimal]$row[0].Timestamp_PerfTime;frequency=[decimal]$row[0].Frequency_PerfTime}
}
function Difference($Before,$After){
    if($Before.name -ne $After.name -or $After.timestamp -le $Before.timestamp -or $After.readOperations -lt $Before.readOperations -or $After.readBytes -lt $Before.readBytes){throw 'Physical performance counter reset or identity change.'}
    [ordered]@{name=$After.name;readOperations=$After.readOperations-$Before.readOperations;readBytes=$After.readBytes-$Before.readBytes;windowSeconds=($After.timestamp-$Before.timestamp)/$After.frequency}
}
try{
    if($Action -eq 'Run'){
        if(Test-Path -LiteralPath $stateFile){throw 'A hardware benchmark already exists; inspect it before resuming.'}
        $hdd=Get-Partition -DriveLetter D|Get-Disk
        $nvme=Get-Partition -DriveLetter M|Get-Disk
        if($hdd.IsBoot -or $hdd.IsSystem -or $hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $nvme.FriendlyName -notmatch '980 PRO' -or $hdd.Number -eq $nvme.Number){throw 'Expected host HDD/NVMe identity mismatch.'}
        if((Get-Volume -DriveLetter D).SizeRemaining -lt 5GB){throw 'Insufficient free HDD space for the owned test file.'}
        $run=[Guid]::NewGuid().ToString('N')
        $state=[ordered]@{at=[DateTime]::UtcNow.ToString('o');vmId=$vm.Id.ToString();phase='planned';error=$null;runId=$run;guestDirectory=('C:\SuperHDDLab\hardware-run-'+$run);hostDirectory=(Join-Path $lab ('hardware-run-'+$run));hddFile=('D:\SuperHDD-Benchmark-'+$run+'.vhdx');hostHdd=[ordered]@{number=$hdd.Number;name=$hdd.FriendlyName;serial=$hdd.SerialNumber;uniqueId=$hdd.UniqueId};hostNvme=[ordered]@{number=$nvme.Number;name=$nvme.FriendlyName;serial=$nvme.SerialNumber;uniqueId=$nvme.UniqueId};hostDriverInstalled=$false;hostSecurityChanged=$false;dedicatedRamPayloadBytes=0;cacheBudgetBytes=768MB;phases=[ordered]@{}}
        New-Item -ItemType Directory -Path $state.hostDirectory|Out-Null
        $state.fixtureBlocks=$FixtureBlocks
        Save-State
    }else{
        $previous=Get-Content -LiteralPath $stateFile -Raw|ConvertFrom-Json -AsHashtable
        $state=$previous
        if($state.phase -eq 'completed'){throw 'This run has completed; retain its report and start a fresh owned run.'}
        if($state.vmId -ne $vm.Id.ToString() -or $state.hddFile -ne ('D:\SuperHDD-Benchmark-'+$state.runId+'.vhdx') -or $state.guestDirectory -ne ('C:\SuperHDDLab\hardware-run-'+$state.runId)){throw 'Recorded benchmark ownership mismatch.'}
        $state.error=$null
        $state.Remove('errorDetail');Save-State
    }
    foreach($identity in @($state.hostHdd,$state.hostNvme)){
        $current=Get-Disk -Number $identity.number
        if($current.UniqueId -ne $identity.uniqueId -or $current.SerialNumber -ne $identity.serial){throw 'Host physical disk identity changed.'}
    }
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    $session=New-PSSession -VMId $vm.Id -Credential $credential
    $guestScript=Join-Path $root 'scripts\kernel-hardware-guest.ps1'
    Copy-VMFile -VM $vm -SourcePath (Join-Path $root 'target\release\examples\raw_benchmark.exe') -DestinationPath 'C:\SuperHDDLab\raw_benchmark.exe' -CreateFullPath -FileSource Host -Force
    $loaded=Invoke-Command -Session $session -ScriptBlock {(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDRawLab'").State -eq 'Running'}
    $helperAction=if($loaded){'Helpers'}else{'ResumeVolumes'}
    Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList $helperAction,$vm.Id.ToString()|Out-Null
    $state.driver=Invoke-Command -Session $session -ScriptBlock {[pscustomobject]@{sha256=(Get-FileHash C:\SuperHDDLab\driver.sys).Hash;signature=(Get-AuthenticodeSignature C:\SuperHDDLab\driver.sys).Status.ToString()}}
    Save-State
    if(-not $state.guestDisk){
        if(Test-Path -LiteralPath $state.hddFile){throw 'Unrecorded HDD fixture exists; refusing replacement.'}
        $before=@(Invoke-Command -Session $session -ScriptBlock {Get-Disk|Select-Object Number,UniqueId})
        New-VHD -Path $state.hddFile -Dynamic -SizeBytes 2GB|Out-Null
        Add-VMHardDiskDrive -VM $vm -ControllerType SCSI -Path $state.hddFile
        Start-Sleep -Seconds 3
        $new=@(Invoke-Command -Session $session -ScriptBlock {Update-HostStorageCache;Get-Disk|Select-Object Number,UniqueId,Size,BusType}|Where-Object {$_.UniqueId -notin $before.UniqueId})
        if($new.Count -ne 1 -or $new[0].Size -ne 2GB){throw 'Cannot identify the newly attached disposable disk.'}
        $state.guestDisk=[ordered]@{number=$new[0].Number;uniqueId=$new[0].UniqueId}
        $state.phase='hdd-vhd-attached';Save-State
    }
    $arguments=@($null,$vm.Id.ToString(),[int]$state.guestDisk.number,[string]$state.guestDisk.uniqueId,[string]$state.guestDirectory,[int]$state.fixtureBlocks)
    if(-not $state.layout){
        $arguments[0]='Prepare'
        $state.layout=Invoke-Command -Session $session -FilePath $guestScript -ArgumentList $arguments
        $state.phase='native-hdd-source-prepared';Save-State
    }
    if(-not $state.fixture){
        $arguments[0]='Fixture'
        $state.fixture=Invoke-Command -Session $session -FilePath $guestScript -ArgumentList $arguments
        $state.phase='fixture-written';Save-State
    }
    foreach($phase in @('Baseline','Cold','Warm')){
        if($state.phases.Contains($phase)){continue}
        $state.phase='measuring-'+$phase.ToLowerInvariant();Save-State
        Start-Sleep -Seconds 2
        $hddBefore=Snapshot $state.hostHdd.number;$ssdBefore=Snapshot $state.hostNvme.number
        $arguments[0]=$phase
        $reading=Invoke-Command -Session $session -FilePath $guestScript -ArgumentList $arguments
        $hddAfter=Snapshot $state.hostHdd.number;$ssdAfter=Snapshot $state.hostNvme.number
        $state.phases[$phase]=[ordered]@{guest=$reading;physicalHdd=(Difference $hddBefore $hddAfter);physicalNvme=(Difference $ssdBefore $ssdAfter);counterScope='Whole physical disk during the phase, including other processes; not a per-file ETW trace.'}
        Save-State
        if(-not $reading.passed -or $reading.errorsDelta){throw "$phase failed data/driver integrity checks."}
    }
    $state.phase='cleaning-owned-fixture';Save-State
    $arguments[0]='Cleanup'
    $state.cleanup=Invoke-Command -Session $session -FilePath $guestScript -ArgumentList $arguments
    $attachment=@($vm|Get-VMHardDiskDrive|Where-Object {$_.Path -eq $state.hddFile})
    if($attachment.Count -ne 1 -or -not $state.cleanup.hddGuestDiskOffline){throw 'Owned attachment cleanup guard failed.'}
    $attachment[0]|Remove-VMHardDiskDrive
    if((Get-VHD -Path $state.hddFile).Attached){throw 'Owned HDD VHD is still attached.'}
    if([IO.Path]::GetFullPath($state.hddFile) -ne ('D:\SuperHDD-Benchmark-'+$state.runId+'.vhdx')){throw 'HDD fixture cleanup path mismatch.'}
    Remove-Item -LiteralPath $state.hddFile
    $state.hostHddFileRemoved= -not(Test-Path -LiteralPath $state.hddFile)
    $state.speedRatio=$state.phases.Warm.guest.iops/$state.phases.Baseline.guest.iops
    $state.passed=$state.phases.Baseline.guest.sha256 -eq $state.phases.Cold.guest.sha256 -and $state.phases.Cold.guest.sha256 -eq $state.phases.Warm.guest.sha256 -and $state.phases.Baseline.physicalHdd.readOperations -gt 0 -and $state.phases.Warm.guest.sourceReadBytesDelta -eq 0 -and $state.phases.Warm.guest.ssdReadBytesDelta -gt 0
    $state.phase='completed';Save-State
}catch{
    if($state){$state.error=$_.Exception.Message;$state.errorDetail=$_.ScriptStackTrace;Save-State}
    else{throw}
}finally{
    if($session){
        if($state.hostDirectory){Copy-Item -FromSession $session -Path ($state.guestDirectory+'\*') -Destination $state.hostDirectory -ErrorAction SilentlyContinue}
        Remove-PSSession -Session $session -ErrorAction SilentlyContinue
    }
    if($runLock){$runLock.Dispose()}
}
