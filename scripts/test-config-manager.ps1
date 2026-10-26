$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$client=Join-Path $workspace 'target\release\examples\library_client.exe'
$run=[Guid]::NewGuid().ToString('N');$checks=[ordered]@{};$items=@()
$evidence=Join-Path $workspace ('artifacts\config-cache\manager\'+$run);[IO.Directory]::CreateDirectory($evidence)|Out-Null
function Call-Client([string[]]$arguments){$out=& $client @arguments 2>&1;if($LASTEXITCODE){throw ($out -join [Environment]::NewLine)};($out -join [Environment]::NewLine)|ConvertFrom-Json}
function Config($command){$p=Join-Path $evidence 'command.json';$command|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $p;Call-Client @('config',$p)}
function State{Call-Client @('state')}
function Row($state,$id){@($state.apps|Where-Object id -eq $id)[0]}
function ConfigRow($state,$letter){@($state.configurations|Where-Object sourceLetter -eq $letter)[0]}
function Save-Config($c){$copy=@{};foreach($k in @('id','name','sourceDiskId','sourceLetter','ssdDiskId','ssdLetter','budgetBytes')){$copy[$k]=$c.$k};Config @{operation='save';configuration=$copy}}
function Reject([scriptblock]$action,[string]$code){try{& $action|Out-Null}catch{if($_.Exception.Message -like ('*'+$code+'*')){return};throw};throw ('Expected rejection: '+$code)}
function Journal($item){Get-Content -LiteralPath (Join-Path $workspace ('artifacts\library-runtime\'+$item.id+'\current.json')) -Raw|ConvertFrom-Json}
function File-Check($item){$expected='original-'+$item.seed;if([IO.File]::ReadAllText((Join-Path $item.source 'nested\data.txt')) -ne $expected){throw 'Original nested data changed'}}
function Budget-Check($state){foreach($c in $state.configurations){$sum=($state.apps|Where-Object configurationId -eq $c.id|Measure-Object allocationBytes -Sum).Sum;if($sum -gt $c.budgetBytes){throw 'Configuration budget exceeded'}}}
$before=State
if(@($before.apps|Where-Object phase -ne stopped).Count){throw 'Stop actual game before manager proof'}
$original=ConfigRow $before 'D:';if(-not $original){throw 'Expected migrated D configuration'}
$devices=Get-Content (Join-Path $workspace 'artifacts\diagnostics\configuration-devices.json') -Raw|ConvertFrom-Json
$edisk=@($devices.disks|Where-Object {@($_.volumes.letter) -contains 'E:'})[0];$fdisk=@($devices.disks|Where-Object {@($_.volumes.letter) -contains 'F:'})[0]
foreach($seed in @(1,2,3,4)){
 $letter=if($seed -eq 2){'E:'}else{'D:'};$source=$letter+'\SuperHDD-ConfigLab-'+$run+'\App'+$seed
 [IO.Directory]::CreateDirectory((Join-Path $source 'nested'))|Out-Null;[IO.File]::WriteAllText((Join-Path $source 'nested\data.txt'),('original-'+$seed));[IO.File]::WriteAllBytes((Join-Path $source 'asset.bin'),[byte[]](0..255)*256)
 $sha=[Security.Cryptography.SHA256]::Create();try{$id=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()};$items+=@{source=$source;id=$id;seed=$seed}
}
$result=[ordered]@{runId=$run;passed=$false;checks=$checks;error=$null;fixtures=$items}
try{
 $d=$original.PSObject.Copy();$d.budgetBytes=2200043520;$state=Save-Config $d
 $e=[pscustomobject]@{id='';name='E to F test';sourceDiskId=$edisk.id;sourceLetter='E:';ssdDiskId=$fdisk.id;ssdLetter='F:';budgetBytes=3300065280}
 $state=Save-Config $e;$e=ConfigRow $state 'E:';$d=ConfigRow $state 'D:'
 $a=$items[0];$b=$items[1];$c=$items[2];$identity=$items[3]
 foreach($item in @($a,$b,$c)){$state=Call-Client @('start',$item.source);Budget-Check $state;File-Check $item}
 if((Journal $a).cacheDrive -ne 'M:' -or (Journal $b).cacheDrive -ne 'F:'){throw 'Wrong SSD selected'}
 if((Row $state $a.id).allocationBytes -ne (Row $state $c.id).allocationBytes -or (Row $state $b.id).allocationBytes -ne $e.budgetBytes){throw 'Independent budgets failed'}
 $checks.twoConfigurationsOnTwoSsds=$true;$checks.sharesOnlyWithinConfiguration=$true
 $duplicate=$d.PSObject.Copy();$duplicate.id='';Reject {Save-Config $duplicate} 'configSourceInUse';$checks.duplicateSourceRejected=$true
 $swap=$e.PSObject.Copy();$swap.ssdDiskId=$original.ssdDiskId;$swap.ssdLetter='M:';Reject {Save-Config $swap} 'configStopFirst';$checks.activeDriveChangeRejected=$true
 $small=$d.PSObject.Copy();$small.budgetBytes=1100021760;Reject {Save-Config $small} 'libraryBudgetTooSmall';$checks.tooSmallSharedBudgetRejected=$true
 $e.budgetBytes=2200043520;$state=Save-Config $e;Budget-Check $state;if((Row $state $a.id).allocationBytes -ne 1100021760){throw 'Other configuration resized'};$checks.liveResizeIsIndependent=$true
 $d.budgetBytes=3300065280;[void](Save-Config $d)
 $storePath=Join-Path $workspace 'artifacts\library-runtime\library.json';$saved=[IO.File]::ReadAllBytes($storePath)
 try{$store=[Text.Encoding]::UTF8.GetString($saved)|ConvertFrom-Json;($store.configurations|Where-Object id -eq $d.id).sourceVolumeId='wrong-volume';$store|ConvertTo-Json -Depth 12|Set-Content -LiteralPath $storePath;Reject {Call-Client @('start',$identity.source)} 'configDeviceChanged'}finally{[IO.File]::WriteAllBytes($storePath,$saved)}
 File-Check $identity;$checks.volumeIdentityChangeRejected=$true
 $d.budgetBytes=2200043520;[void](Save-Config $d)
 $cacheA=(Journal $a).cache;$cacheC=(Journal $c).cache
 $state=Config @{operation='stop';id=$d.id};if((Row $state $a.id).phase -ne 'stopped' -or (Row $state $c.id).phase -ne 'stopped' -or (Row $state $b.id).phase -ne 'active' -or (Test-Path $cacheA) -or (Test-Path $cacheC)){throw 'Configuration stop crossed boundaries'};File-Check $a;File-Check $c;$checks.stopRestoresOwnFoldersOnly=$true
 $d.ssdDiskId=$e.ssdDiskId;$d.ssdLetter=$e.ssdLetter;$state=Save-Config $d;$state=Call-Client @('start',$a.source);if((Journal $a).cacheDrive -ne 'F:'){throw 'Stopped configuration SSD change failed'};Budget-Check $state;$checks.stoppedConfigurationCanChangeSsd=$true
 [void](Call-Client @('pause',$a.id));$state=State;if((Row $state $b.id).phase -ne 'active'){throw 'Pause affected other config'};[void](Call-Client @('resume',$a.id));$checks.sharedSsdConfigurationsStayIndependent=$true
 [void](Config @{operation='stop';id=$d.id});$d.ssdDiskId=$original.ssdDiskId;$d.ssdLetter=$original.ssdLetter;[void](Save-Config $d);$state=Call-Client @('start',$a.source)
 $cacheB=(Journal $b).cache;$state=Config @{operation='remove';id=$e.id};if((ConfigRow $state 'E:') -or (Row $state $b.id).phase -ne 'stopped' -or (Row $state $a.id).phase -ne 'active' -or (Test-Path $cacheB)){throw 'Configuration removal affected other cache'};File-Check $b;$checks.removeReleasesCachePreservesOtherConfig=$true
 $checks.originalProfilesRemain=$state.apps.id -contains 'f0cacf2ec860a8fefeeb1bfe1c254f0a';if(-not $checks.originalProfilesRemain){throw 'Original game profile removed'}
 [void](Config @{operation='stop';id=$d.id});foreach($item in $items){$state=State;if(Row $state $item.id){[void](Call-Client @('remove',$item.id))};File-Check $item}
 [void](Save-Config $original);$result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
 foreach($item in $items){try{$state=State;$row=Row $state $item.id;if($row -and $row.phase -ne 'stopped'){[void](Call-Client @('stop',$item.id))};if(Row (State) $item.id){[void](Call-Client @('remove',$item.id))}}catch{$result['cleanup_'+$item.id]=$_.Exception.Message}}
 try{$state=State;$other=ConfigRow $state 'E:';if($other){[void](Config @{operation='remove';id=$other.id})};[void](Save-Config $original)}catch{$result.cleanupConfiguration=$_.Exception.Message}
 $result|ConvertTo-Json -Depth 10|Set-Content (Join-Path $workspace 'artifacts\config-cache\manager-proof.json')
}
$result|ConvertTo-Json -Depth 10
if(-not $result.passed){throw 'Configuration manager proof failed'}
