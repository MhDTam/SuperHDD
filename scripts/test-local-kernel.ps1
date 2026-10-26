param(
    [ValidateSet('Prepare','Run','Cleanup')][string]$Action='Prepare',
    [ValidatePattern('^[a-f0-9]{32}$')][string]$RunId
)
$ErrorActionPreference='Stop'
$workspace=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$runtime=Join-Path $workspace 'artifacts\runtime'
$private=Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'SuperHDDHostTest'
$install=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Super HDD Host Test'
$serviceName='SuperHDDHostTest'
$stateFile=Join-Path $private 'state.json'
$boot=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
function Assert-NoLinks([string]$Path){
    $item=Get-Item -LiteralPath $Path -Force
    while($item){
        if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'Reparse points are not allowed in host-test paths.'}
        $item=if($item -is [IO.FileInfo]){$item.Directory}else{$item.Parent}
    }
}
function Save-State {
    $temporary=Join-Path $private 'state.new.json'
    $state|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $temporary -Encoding utf8NoBOM
    Move-Item -LiteralPath $temporary -Destination $stateFile -Force
}
function Protect-Directory([string]$Path,[bool]$Readers=$false){
    $acl=[Security.AccessControl.DirectorySecurity]::new()
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    $acl.SetAccessRuleProtection($true,$false)
    foreach($sid in @('S-1-5-18','S-1-5-32-544')){
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
    }
    if($Readers){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'),'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'))}
    Set-Acl -LiteralPath $Path -AclObject $acl
}
function Hash([string]$Path){(Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash}
function Get-Parents {
    $hdd=Get-Partition -DriveLetter D|Get-Disk
    $ssd=Get-Partition -DriveLetter M|Get-Disk
    $dv=Get-Volume -DriveLetter D
    $mv=Get-Volume -DriveLetter M
    if($hdd.IsBoot -or $hdd.IsSystem -or $hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $ssd.FriendlyName -notmatch '980 PRO' -or $ssd.IsBoot -or $ssd.IsSystem -or $hdd.Number -eq $ssd.Number -or $dv.FileSystem -ne 'NTFS' -or $mv.FileSystem -ne 'NTFS'){throw 'The expected D HDD / M SSD identity is unavailable.'}
    [pscustomobject]@{sourceDisk=[string]$hdd.UniqueId;sourceVolume=[string]$dv.UniqueId;cacheDisk=[string]$ssd.UniqueId;cacheVolume=[string]$mv.UniqueId;hddFree=$dv.SizeRemaining;ssdFree=$mv.SizeRemaining}
}
function Assert-Parents($Expected){
    $now=Get-Parents
    foreach($key in @('sourceDisk','sourceVolume','cacheDisk','cacheVolume')){
        if($now.$key -ne $Expected.$key){throw 'Host parent identity changed; preserve owned files.'}
    }
}
function Assert-StatePrivate {
    Assert-NoLinks $private
    $acl=Get-Acl -LiteralPath $private
    $owner=$acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
    $rules=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
    if(-not $acl.AreAccessRulesProtected -or $owner -notin 'S-1-5-18','S-1-5-32-544' -or $rules.Count -ne 2){throw 'Host-test state is not private.'}
    foreach($r in $rules){if($r.IdentityReference.Value -notin 'S-1-5-18','S-1-5-32-544' -or $r.AccessControlType -ne 'Allow' -or $r.FileSystemRights -ne 'FullControl'){throw 'Host-test state permissions changed.'}}
}
function Assert-Source($State){
    $vhd=Get-VHD -Path $sourcePath
    if(-not $vhd.Attached -or $vhd.VhdType -ne 'Fixed' -or $vhd.Size -ne 1GB){throw 'Owned source VHD identity changed.'}
    $disk=$vhd|Get-Disk
    $volume=Get-Volume -DriveLetter S
    $partition=Get-Partition -DriveLetter S
    if($disk.UniqueId -ne $State.sourceDisk -or $partition.DiskNumber -ne $disk.Number -or $disk.IsBoot -or $disk.IsSystem -or [int]$disk.BusType -ne 15 -or $volume.UniqueId -ne $State.sourceVolume -or $volume.FileSystemLabel -ne ('SHDD-L-'+$RunId.Substring(0,8))){throw 'Only the new owned S test volume may be used.'}
    $disk
}
function Remove-OwnedTest {
    Assert-StatePrivate
    Assert-Parents $state.parents
    if(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -and ([IO.Path]::GetDirectoryName($_.ExecutablePath) -eq $install)}){throw 'An owned test process remains; preserve storage until it exits normally.'}
    $probe=Join-Path $install 'host_session_probe.exe'
    $cache=Join-Path $runRoot 'cache'
    if(Test-Path -LiteralPath $cache){
        if((Hash $probe) -ne $state.hashes.'host_session_probe.exe'){throw 'Protected ownership helper changed.'}
        & $probe --remove-owned-cache $cache *> (Join-Path $runRoot 'cleanup-cache.log')
        if($LASTEXITCODE){throw 'Owned cache could not be removed normally; preserve source and cache.'}
    }
    if(Test-Path -LiteralPath $sourcePath){
        Assert-NoLinks $sourcePath
        $vhd=Get-VHD -Path $sourcePath
        if($vhd.DiskIdentifier.ToString() -ne $state.sourceVhdId -or $vhd.Size -ne 1GB -or $vhd.VhdType -ne 'Fixed'){throw 'Refusing cleanup of an unowned VHDX.'}
        if($vhd.Attached){
            if($state.sourceVolume){$null=Assert-Source $state}else{
                $disk=$vhd|Get-Disk
                if($state.enrolled -or $disk.IsBoot -or $disk.IsSystem -or [int]$disk.BusType -ne 15 -or $disk.Size -ne 1GB){throw 'Incomplete source cannot be verified; preserve it.'}
            }
            if($state.enrolled){
                if((Hash $probe) -ne $state.hashes.'host_session_probe.exe'){throw 'Protected status helper changed.'}
                $status=& $probe --status 2> (Join-Path $runRoot 'cleanup-status.log')
                if($LASTEXITCODE){throw 'Cannot prove the kernel session is stopped; preserve source and cache.'}
                if(($status|ConvertFrom-Json).cache.attached){throw 'Kernel cache is still attached; preserve source and cache.'}
                [SuperHDDHostPnp]::SetFilter($state.sourceNode,'',$false)
                & pnputil.exe /restart-device $state.sourceNode *> (Join-Path $runRoot 'remove-filter.log')
                if($LASTEXITCODE){throw 'Owned source restart failed; preserve mounted storage.'}
                $state.enrolled=$false;Save-State
            }
            if(Test-Path -LiteralPath (Join-Path $runRoot 'cache\owner.json')){throw 'Owned cache journal remains; complete its normal transactional removal before dismounting the source.'}
            Dismount-VHD -Path $sourcePath
            if((Get-VHD -Path $sourcePath).Attached){throw 'Owned source is still attached.'}
        }elseif($state.enrolled){
            # A cold boot leaves a non-present owned devnode. Open this exact saved
            # instance, including non-present nodes; remove only our own filter.
            [SuperHDDHostPnp]::SetFilter($state.sourceNode,'',$false)
            $state.enrolled=$false;Save-State
        }
        if(Test-Path -LiteralPath (Join-Path $runRoot 'cache\owner.json')){throw 'Cache journal remains; preserve both owned storage files for recovery.'}
        Remove-Item -LiteralPath $sourcePath
        $state.sourceRemoved=$true;Save-State
    }elseif($state.sourceVhdId -and -not $state.sourceRemoved){throw 'Owned source disappeared; inspect preserved state before cleanup.'}
    $registered=Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDHostTest'"
    if($registered){
        if($registered.PathName.Trim('"') -ne (Join-Path $install 'driver.sys')){throw 'Kernel registration belongs to another image.'}
        & sc.exe delete $serviceName *> (Join-Path $runRoot 'delete-driver.log')
        if($LASTEXITCODE){throw 'Cannot delete the owned demand-start registration.'}
    }
    foreach($name in @('driver.sys','SuperHDDService.exe','host_session_probe.exe')){
        $file=Join-Path $install $name
        if(Test-Path -LiteralPath $file){if((Hash $file) -ne $state.hashes.$name){throw 'Installed test image changed; preserve it.'};Remove-Item -LiteralPath $file}
    }
    if(Test-Path -LiteralPath $install){Remove-Item -LiteralPath $install} # Empty directory only.
    $state.phase='cleaned';Save-State
    Copy-Item -LiteralPath $stateFile -Destination (Join-Path $runRoot 'final-state.json')
    Remove-Item -LiteralPath $stateFile
    Remove-Item -LiteralPath $private # Empty directory only.
    [pscustomobject]@{cleaned=$true;runId=$RunId;hostBootSettingsChanged=$false;note='PnP may keep an unloaded image reference until the next normal reboot; no reboot was requested.'}
}
if($Action -eq 'Prepare'){
    if($RunId){throw 'Prepare generates a fresh run ID.'}
    $RunId=[Guid]::NewGuid().ToString('N')
    $runRoot=Join-Path $runtime ('host-local-'+$RunId)
    $sourcePath='D:\SuperHDD-LocalSource-'+$RunId+'.vhdx'
    $parents=Get-Parents
    if($parents.hddFree -lt 5GB -or $parents.ssdFree -lt 2GB){throw 'Insufficient free space for the new owned files.'}
    if((Test-Path -LiteralPath $private) -or (Test-Path -LiteralPath $install) -or (Test-Path -LiteralPath $sourcePath)){throw 'An earlier host test must be inspected/cleaned first.'}
    $files=@{'driver.sys'=(Join-Path $runtime 'kernel-lab\signed-driver.sys');'SuperHDDService.exe'=(Join-Path $workspace 'target\release\SuperHDDService.exe');'host_session_probe.exe'=(Join-Path $workspace 'target\release\examples\host_session_probe.exe')}
    $signature=Get-AuthenticodeSignature -LiteralPath $files.'driver.sys'
    if((Hash $files.'driver.sys') -ne 'EA77A8A86DB028ED9E3F000D9417C6FBE59D96ED143BC233FD1CAF61CF92E9CA' -or $signature.SignerCertificate.Subject -ne 'CN=SuperHDD Disposable VM 0150b344-482c-4758-b3e4-9f4bbf2ec015' -or $signature.SignerCertificate.NotAfter -le (Get-Date)){throw 'The previously tested embedded-signed developer image is unavailable.'}
    Assert-NoLinks $runtime
    New-Item -ItemType Directory -Path $runRoot|Out-Null
    $package=Join-Path $runRoot 'package'
    New-Item -ItemType Directory -Path $package|Out-Null
    $hashes=[ordered]@{}
    foreach($name in $files.Keys){Copy-Item -LiteralPath $files[$name] -Destination (Join-Path $package $name);$hashes[$name]=Hash (Join-Path $package $name)}
    $plan=[ordered]@{schema=1;runId=$RunId;machineSha256=$boot.machineSha256;parents=$parents;hashes=$hashes;sourceVhdBytes=1GB;cacheBudgetBytes=256MB;scope='Native host kernel test on one NEW fixed VHDX; existing D/E volumes are never enrolled';bootSettingsChanged=$false;needsSecureBootOff=$true;needsActiveTestSigning=$true;priorBoot=$boot}
    $plan|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $runRoot 'plan.json') -Encoding utf8NoBOM
    [pscustomobject]@{prepared=$true;runId=$RunId;plan=(Join-Path $runRoot 'plan.json');driverInstalled=$false;storageAllocated=$false;boot=$boot}
    return
}
# Fail before elevation, allocation, registration or enrollment. Stored BCD text
# is insufficient: query the kernel's currently active test-signing state.
if($Action -eq 'Run' -and ($boot.secureBootEnabled -or -not $boot.testSigningActive)){
    throw 'LOCAL_BOOT_PREREQUISITE: Secure Boot is enabled or test signing is not active. No driver, source VHDX or cache was created. No boot setting was changed.'
}
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'This action requires an administrator after its boot prerequisites are satisfied.'}
if($Action -eq 'Cleanup'){
    Assert-StatePrivate
    $state=Get-Content -LiteralPath $stateFile -Raw|ConvertFrom-Json
    if($RunId -and $RunId -ne $state.runId){throw 'Cleanup run ID mismatch.'}
    $RunId=$state.runId
}
if($RunId -notmatch '^[a-f0-9]{32}$'){throw 'An exact prepared run ID is required.'}
$runRoot=Join-Path $runtime ('host-local-'+$RunId)
$sourcePath='D:\SuperHDD-LocalSource-'+$RunId+'.vhdx'
Assert-NoLinks $runRoot
Add-Type -LiteralPath (Join-Path $PSScriptRoot 'host-test-pnp.cs')
$runLock=[IO.File]::Open((Join-Path $runtime 'kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try{
    if($Action -eq 'Cleanup'){
        if($state.machineSha256 -ne $boot.machineSha256 -or $state.schema -ne 1){throw 'Private state machine/schema mismatch.'}
        Remove-OwnedTest
        return
    }
    $plan=Get-Content -LiteralPath (Join-Path $runRoot 'plan.json') -Raw|ConvertFrom-Json
    if($plan.runId -ne $RunId -or $plan.machineSha256 -ne $boot.machineSha256 -or $plan.schema -ne 1 -or $plan.cacheBudgetBytes -ne 256MB -or $plan.sourceVhdBytes -ne 1GB){throw 'Prepared plan identity mismatch.'}
    Assert-Parents $plan.parents
    Assert-NoLinks ([Environment]::GetFolderPath('ProgramFiles'))
    Assert-NoLinks ([Environment]::GetFolderPath('CommonApplicationData'))
    if((Test-Path -LiteralPath $sourcePath) -or (Test-Path -LiteralPath $private) -or (Test-Path -LiteralPath $install) -or (Get-Volume -DriveLetter S -ErrorAction SilentlyContinue)){throw 'Existing source, reserved letter or host-test installation must be preserved.'}
    if(Get-CimInstance Win32_SystemDriver|Where-Object {$_.Name -like '*SuperHDD*'}){throw 'An existing SuperHDD kernel registration must be inspected first.'}
    $files=@('driver.sys','SuperHDDService.exe','host_session_probe.exe')
    foreach($name in $files){$file=Join-Path $runRoot ('package\'+$name);Assert-NoLinks $file;if((Hash $file) -ne $plan.hashes.$name){throw 'Prepared binary hash changed.'}}
    if($plan.hashes.'driver.sys' -ne 'EA77A8A86DB028ED9E3F000D9417C6FBE59D96ED143BC233FD1CAF61CF92E9CA'){throw 'Unexpected kernel image.'}
    $facts=& (Join-Path $workspace 'target\release\examples\local_preflight.exe')|ConvertFrom-Json
    if($LASTEXITCODE -or -not $facts.readOnly){throw 'Native read-only parent preflight failed.'}
    New-Item -ItemType Directory -Path $private|Out-Null
    Protect-Directory $private
    Assert-StatePrivate
    $state=[pscustomobject]@{schema=1;runId=$RunId;machineSha256=$boot.machineSha256;parents=$plan.parents;hashes=$plan.hashes;sourceVhdId=$null;sourceRemoved=$false;sourceDisk=$null;sourceVolume=$null;sourceNode=$null;enrolled=$false;phase='prepared';error=$null}
    Save-State
    New-Item -ItemType Directory -Path $install|Out-Null
    Protect-Directory $install $true
    Protect-Directory $runRoot $true
    foreach($name in $files){Copy-Item -LiteralPath (Join-Path $runRoot ('package\'+$name)) -Destination (Join-Path $install $name);if((Hash (Join-Path $install $name)) -ne $state.hashes.$name){throw 'Protected image copy mismatch.'}}
    New-VHD -Path $sourcePath -Fixed -SizeBytes 1GB|Out-Null
    $state.sourceVhdId=(Get-VHD -Path $sourcePath).DiskIdentifier.ToString();$state.phase='source-created';Save-State
    $vhd=Mount-VHD -Path $sourcePath -PassThru
    $disk=$vhd|Get-Disk
    if($disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 1GB -or [int]$disk.BusType -ne 15 -or $disk.PartitionStyle -ne 'RAW' -or @($disk|Get-Partition -ErrorAction SilentlyContinue).Count){throw 'New exact VHDX is not the expected empty file-backed disk.'}
    if($disk.IsOffline){$disk|Set-Disk -IsOffline $false}
    if($disk.IsReadOnly){$disk|Set-Disk -IsReadOnly $false}
    $disk|Initialize-Disk -PartitionStyle MBR -PassThru|New-Partition -UseMaximumSize -DriveLetter S|Format-Volume -FileSystem NTFS -NewFileSystemLabel ('SHDD-L-'+$RunId.Substring(0,8)) -Confirm:$false|Out-Null
    $state.sourceDisk=[string]((Get-VHD -Path $sourcePath)|Get-Disk).UniqueId
    $state.sourceVolume=[string](Get-Volume -DriveLetter S).UniqueId
    $null=Assert-Source $state
    $pdo=[SuperHDDHostPnp]::Device('S:')
    $nodes=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $pdo})
    if($nodes.Count -ne 1){throw 'Owned source devnode is ambiguous.'}
    $state.sourceNode=$nodes[0].InstanceId;$state.phase='source-initialized';Save-State
    & sc.exe create $serviceName type= kernel start= demand binPath= (Join-Path $install 'driver.sys') *> (Join-Path $runRoot 'create-driver.log')
    if($LASTEXITCODE){throw 'Cannot register the owned demand-start kernel test.'}
    # Persist intent first. Cleanup removes only our exact saved node/property.
    $state.enrolled=$true;$state.phase='enrolling';Save-State
    [SuperHDDHostPnp]::SetFilter($state.sourceNode,$pdo,$true)
    & pnputil.exe /restart-device $state.sourceNode *> (Join-Path $runRoot 'load-driver.log')
    if($LASTEXITCODE){throw 'Windows did not restart the owned source with the test filter.'}
    $state.phase='driver-loaded';Save-State
    $cache=Join-Path $runRoot 'cache'
    New-Item -ItemType Directory -Path $cache|Out-Null
    $request=Join-Path $runRoot 'request.json'
    @{sourceVolume='S:';containerRoot=$cache;budgetBytes=256MB;acknowledgedDisposableSource=$true;isolatedLab=$null}|ConvertTo-Json|Set-Content -LiteralPath $request -Encoding utf8NoBOM
    & (Join-Path $install 'host_session_probe.exe') (Join-Path $install 'SuperHDDService.exe') $request (Join-Path $runRoot 'probe.json') *> (Join-Path $runRoot 'probe.log')
    if($LASTEXITCODE){throw 'Native host data-cycle check failed. Preserve the owned test state for recovery.'}
    $report=Get-Content -LiteralPath (Join-Path $runRoot 'probe.json') -Raw|ConvertFrom-Json
    if(-not $report.passed -or -not $report.ownedFixturesRemoved){throw 'Native host verification/transactional removal is incomplete.'}
    $state.phase='passed';Save-State
    Remove-OwnedTest
    $report
}catch{
    if($state -and (Test-Path -LiteralPath $private)){$state.error=$_.Exception.Message;Save-State}
    throw
}finally{$runLock.Dispose()}
