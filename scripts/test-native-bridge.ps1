$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
    throw 'Native bridge VM test requires an ordinary Windows administrator session; no VM or service was changed.'
}
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$runLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$runId=[Guid]::NewGuid().ToString('N')
$guestRun='C:\SuperHDDLab\native-bridge-'+$runId
$guestBinaries='C:\Program Files\SuperHDDLab\Bridge-'+$runId
$resultPath=Join-Path $lab 'native-bridge.json'
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$runId;phase='starting';passed=$false;error=$null;hostServiceInstalled=$false;vendorUsed=$false}
$session=$null
function Save-Result{$result|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $resultPath -Encoding utf8NoBOM}
try{
    if(Test-Path -LiteralPath $resultPath){Move-Item -LiteralPath $resultPath -Destination (Join-Path $lab ('native-bridge-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmss')+'.json'))}
    $configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $configuration.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Wrong disposable VM.'}
    $result.vmId=$vm.Id.ToString();Save-Result
    if($vm.State -eq 'Off'){Start-VM -VM $vm|Out-Null}
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    for($attempt=0;$attempt -lt 40;$attempt++){
        try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 3}
    }
    if(-not $session){throw 'Disposable guest not ready.'}
    # Preserve exact earlier probe images before treating any rebuilt tool as a
    # current artifact. Old kernel/benchmark runs remain bound to these hashes.
    $historical=Join-Path $lab 'historical-tools'
    New-Item -ItemType Directory -Path $historical -Force|Out-Null
    $result.historicalTools=@()
    $baseline=Get-Content -LiteralPath (Join-Path $root 'artifacts\kernel-tool-baseline.json') -Raw|ConvertFrom-Json
    foreach($entry in $baseline.tools){
        if($entry.name -notin 'raw_lab.exe','pnp_probe.exe','raw_benchmark.exe'){throw 'Unexpected historical tool name.'}
        $guestFile='C:\SuperHDDLab\'+$entry.name
        $guestHash=Invoke-Command -Session $session -ArgumentList $guestFile -ScriptBlock {param($File)(Get-FileHash -LiteralPath $File).Hash}
        if($guestHash -ne $entry.sha256){throw 'Historical guest tool does not match its recorded exact-image hash.'}
        $archived=Join-Path $historical $entry.name
        if(-not(Test-Path -LiteralPath $archived)){Copy-Item -FromSession $session -LiteralPath $guestFile -Destination $archived}
        if((Get-FileHash -LiteralPath $archived).Hash -ne $entry.sha256){throw 'Historical tool archive mismatch.'}
        $result.historicalTools+=@{name=$entry.name;sha256=$entry.sha256;path=$archived}
    }
    $result.phase='preparing-owned-volumes';Save-Result
    $result.volumes=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()
    $result.preparation=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries -ScriptBlock {
        param($Run,$Binaries)
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or $Run -notmatch '^C:\\SuperHDDLab\\native-bridge-[a-f0-9]{32}$' -or $Binaries -notmatch '^C:\\Program Files\\SuperHDDLab\\Bridge-[a-f0-9]{32}$'){throw 'Guest/path identity failed.'}
        if(Get-Service -Name SuperHDDService -ErrorAction SilentlyContinue){throw 'Refusing to replace an existing guest service.'}
        New-Item -ItemType Directory -Path $Run,$Binaries -ErrorAction Stop|Out-Null
        [pscustomobject]@{driverHash=(Get-FileHash C:\SuperHDDLab\driver.sys).Hash;signature=(Get-AuthenticodeSignature C:\SuperHDDLab\driver.sys).Status.ToString()}
    }
    if($result.preparation.signature -ne 'Valid' -or $result.preparation.driverHash -ne (Get-FileHash (Join-Path $lab 'signed-driver.sys')).Hash){throw 'Wrong or unsigned guest driver.'}
    $files=@{'target\release\SuperHDDService.exe'='SuperHDDService.exe';'target\release\examples\native_bridge_probe.exe'='native_bridge_probe.exe'}
    $result.binaryHashes=@()
    foreach($file in $files.GetEnumerator()){
        $source=Join-Path $root $file.Key
        Copy-Item -ToSession $session -LiteralPath $source -Destination (Join-Path $guestBinaries $file.Value)
        $result.binaryHashes+=@{name=$file.Value;sha256=(Get-FileHash -LiteralPath $source).Hash}
    }
    $result.phase='native-driver-service-desktop-contract';Save-Result
    $result.probe=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries,$runId -ScriptBlock {
        param($Run,$Binaries,$RunId)
        $servicePath=Join-Path $Binaries 'SuperHDDService.exe'
        $service=New-Service -Name SuperHDDService -BinaryPathName ('"'+$servicePath+'"') -StartupType Manual -DisplayName 'Super HDD native bridge disposable lab'
        $service.Dispose()
        Start-Service -Name SuperHDDService
        & C:\SuperHDDLab\raw_lab.exe configure C:\SuperHDDLab\request.json *> (Join-Path $Run 'configure.log')
        if($LASTEXITCODE){throw 'Guest native-cache configuration failed.'}
        & (Join-Path $Binaries 'native_bridge_probe.exe') ('R:\superhdd-native-bridge-'+$RunId) (Join-Path $Run 'probe.json') *> (Join-Path $Run 'probe.log')
        if($LASTEXITCODE){throw 'Native bridge proof failed; inspect probe.log.'}
        Get-Content -LiteralPath (Join-Path $Run 'probe.json') -Raw|ConvertFrom-Json
    }
    if(-not $result.probe.passed -or -not $result.probe.fixtureRemoved){throw 'Native bridge/data cleanup checks failed.'}
    $result.passed=$true
}catch{$result.error=$_.Exception.Message;$result.errorDetail=$_.ScriptStackTrace}finally{
    if($session){
        try{
            $result.cleanup=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries -ScriptBlock {
                param($Run,$Binaries)
                $expected=Join-Path $Binaries 'SuperHDDService.exe'
                $installed=Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"
                if($installed){
                    if($installed.PathName.Trim('"') -ne $expected){throw 'Guest service ownership changed; refusing cleanup.'}
                    Stop-Service -Name SuperHDDService -ErrorAction Stop
                    & sc.exe delete SuperHDDService *> (Join-Path $Run 'delete-service.log')
                    if($LASTEXITCODE){throw 'Guest service deletion failed.'}
                }
                & C:\SuperHDDLab\raw_lab.exe disable *> (Join-Path $Run 'disable.log')
                if($LASTEXITCODE){throw 'Guest cache disable failed.'}
                [pscustomobject]@{serviceAbsent=(-not(Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"));cacheDetached=((& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json).cache.attached -eq $false)}
            }
            Copy-Item -FromSession $session -LiteralPath $guestRun -Destination (Join-Path $lab ('native-bridge-'+$runId)) -Recurse
        }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
        Remove-PSSession $session -ErrorAction SilentlyContinue
    }
    $result.passed=[bool]($result.passed -and $result.cleanup.serviceAbsent -and $result.cleanup.cacheDetached)
    $result.phase=if($result.passed){'probe-passed-shutdown-pending'}else{'failed'}
    Save-Result
    $runLock.Dispose()
}
try{
    & (Join-Path $root 'scripts\stop-kernel-lab.ps1')
    $result.finalHost=Get-Content -LiteralPath (Join-Path $lab 'final-host-state.json') -Raw|ConvertFrom-Json
    $result.passed=[bool]($result.passed -and $result.finalHost.passed -and $result.finalHost.vmState -eq 'Off')
    $result.phase=if($result.passed){'completed'}else{'failed'}
}catch{$result.shutdownError=$_.Exception.Message;$result.passed=$false;$result.phase='failed'}
$result.sourceHashes=@('crates\platform\src\driver.rs','crates\platform\src\raw_lab.rs','crates\platform\src\ipc.rs','crates\platform\src\discover.ps1','crates\platform\src\managed_cache.rs','services\superhdd-service\src\main.rs','services\superhdd-service\src\lab_control.rs','crates\storage-control\src\session.rs','crates\platform\examples\native_bridge_probe.rs'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
Save-Result
if(-not $result.passed){throw (@($result.error,$result.cleanupError,$result.shutdownError,'Native bridge verification did not pass.')|Where-Object {$_}|Select-Object -First 1)}
