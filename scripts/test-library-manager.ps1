$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$client=Join-Path $workspace 'target\release\examples\library_client.exe'
$run=[Guid]::NewGuid().ToString('N');$items=@();$checks=[ordered]@{}
function Call-Library([string[]]$arguments){$out=& $client @arguments 2>&1;if($LASTEXITCODE -ne 0){throw ($out -join [Environment]::NewLine)};($out -join [Environment]::NewLine)|ConvertFrom-Json}
function Id([string]$path){$sha=[Security.Cryptography.SHA256]::Create();try{([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($path.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}}
function Budget-Check($state){if($state.allocatedBytes -gt $state.budgetBytes){throw 'Shared allocation exceeds budget'}}
function Row($state,$id){@($state.apps|Where-Object id -eq $id)[0]}
$before=Call-Library @('state');if(@($before.apps|Where-Object phase -ne 'stopped').Count){throw 'Manager proof requires no live library sessions'}
foreach($index in @(1,2)){$source='D:\SuperHDD-ManagerLab-'+$run+'\App'+$index;[IO.Directory]::CreateDirectory((Join-Path $source 'nested'))|Out-Null;[IO.File]::WriteAllText((Join-Path $source 'nested\data.txt'),('original-'+$index));$items+=@{source=$source;id=(Id $source);value=('original-'+$index)}}
$result=[ordered]@{runId=$run;passed=$false;checks=$checks;error=$null;fixtures=$items}
try{
 [void](Call-Library @('budget','24999952384'))
 $a=$items[0];$b=$items[1]
 $state=Call-Library @('start',$a.source);Budget-Check $state
 if((Row $state $a.id).allocationBytes -ne 24999952384){throw 'First item did not receive the whole budget'}
 $state=Call-Library @('start',$b.source);Budget-Check $state
 $alloc=@($state.apps|Where-Object phase -eq 'active'|Select-Object -ExpandProperty allocationBytes);if($alloc.Count -ne 2 -or $alloc[0] -ne $alloc[1]){throw 'Budget was not divided equally'}
 $checks.twoAppsShareOneBudget=$true
 foreach($app in $items){if([IO.File]::ReadAllText((Join-Path $app.source 'nested\data.txt')) -ne $app.value){throw 'Nested source bytes changed'}}
 $checks.nestedFoldersWork=$true
 $state=Call-Library @('pause',$a.id);if((Row $state $a.id).phase -ne 'paused' -or (Row $state $b.id).phase -ne 'active'){throw 'Independent pause failed'}
 [void](Call-Library @('resume',$a.id));[void](Call-Library @('clear',$a.id));$checks.independentControls=$true
 $state=Call-Library @('budget','2200043520');Budget-Check $state
 if((Row $state $a.id).allocationBytes -ne (Row $state $b.id).allocationBytes){throw 'Resize split mismatch'}
 $checks.liveBudgetResize=$true
 $state=Call-Library @('budget','3300065280');Budget-Check $state
 if((Row $state $a.id).allocationBytes -le 1100021760 -or (Row $state $a.id).allocationBytes -ne (Row $state $b.id).allocationBytes){throw 'Shared budget growth failed'}
 $checks.liveBudgetGrowth=$true
 $rejected=$false;try{[void](Call-Library @('budget','1100021760'))}catch{$rejected=$_.Exception.Message -like '*libraryBudgetTooSmall*'};if(-not $rejected){throw 'Too-small shared budget accepted'};$checks.insufficientBudgetRejected=$true
 $nested=Join-Path $a.source 'nested';$nestedId=Id $nested;$rejected=$false;try{[void](Call-Library @('start',$nested))}catch{$rejected=$_.Exception.Message -like '*libraryOverlap*'};if(-not $rejected){throw 'Overlapping folder accepted'};$checks.overlappingFoldersRejected=$true
 $state=Call-Library @('stop',$a.id);Budget-Check $state
 if((Row $state $b.id).allocationBytes -ne $state.budgetBytes){throw 'Released space not assigned to remaining item'}
 if([IO.File]::ReadAllText((Join-Path $a.source 'nested\data.txt')) -ne $a.value -or [IO.File]::ReadAllText((Join-Path $b.source 'nested\data.txt')) -ne $b.value){throw 'Source restoration affected other app'}
 $checks.stopRestoresOneAndRebalancesOther=$true
 [void](Call-Library @('stop',$b.id))
 foreach($id in @($a.id,$b.id,$nestedId)){[void](Call-Library @('remove',$id))}
 [void](Call-Library @('budget','24999952384'))
 $checks.profilesCanBeRemovedWithoutDeletingSource=$true;$result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
 foreach($app in $items){try{$state=Call-Library @('state');$row=Row $state $app.id;if($row -and $row.phase -ne 'stopped'){[void](Call-Library @('stop',$app.id))}}catch{}}
 $result|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\library-cache\manager-proof.json')
}
$result|ConvertTo-Json -Depth 8
if(-not $result.passed){throw 'Library manager proof failed'}
