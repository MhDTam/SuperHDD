$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$client=Join-Path $workspace 'target\debug\examples\library_client.exe'
$run=[Guid]::NewGuid().ToString('N')
$evidence=Join-Path $workspace ('artifacts\game-scope\'+$run)
[void][IO.Directory]::CreateDirectory($evidence)
$source='D:\SuperHDD-ScopeLab-'+$run
$data=Join-Path $source 'Game\Content\Paks'
$store=Join-Path $workspace 'artifacts\library-runtime\library.json'
$beforeHash=(Get-FileHash -LiteralPath $store).Hash
$checks=New-Object 'System.Collections.Generic.List[string]'
function Call([string[]]$arguments){$out=& $client @arguments 2>&1;if($LASTEXITCODE){throw ($out -join "`n")};($out -join "`n")|ConvertFrom-Json}
function Op($command){$file=Join-Path $evidence command.json;[IO.File]::WriteAllText($file,($command|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));Call @('operate',$file)}
function Check([bool]$ok,[string]$name){if(-not $ok){throw $name};$checks.Add($name)}
function Item($state){@($state.apps|Where-Object id -eq $app.id)[0]}
$sha=[Security.Cryptography.SHA256]::Create()
try{$id=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}
$app=@{id=$id;name='Scope fixture';path=$source;origin='Manual';kind='game';iconDataUri=$null}
$original=[byte[]](0..255)*1024
[void][IO.Directory]::CreateDirectory($data)
[IO.File]::WriteAllBytes((Join-Path $data 'assets.pak'),$original)
[IO.File]::WriteAllText((Join-Path $source 'game.exe'),'Fixture only; never execute')
[IO.File]::WriteAllText((Join-Path $source 'owner.marker'),$run)
$proof=[ordered]@{passed=$false;run=$run;checks=$checks;error=$null;source=$source}
try{
 $state=Op @{operation='modeData';id=$id;app=$app}
 Check ((Item $state).cacheMode -eq 'dataOnly' -and (Item $state).phase -eq 'stopped') 'Selecting data mode is saved without mounting'
 $state=Op @{operation='start';id=$id;app=$app}
 Check ((Item $state).phase -eq 'active' -and (Item $state).cacheMode -eq 'dataOnly') 'Starting the parent game activates its data cache'
 Check (@($state.apps|Where-Object {$_.path.StartsWith($source,[StringComparison]::OrdinalIgnoreCase)}).Count -eq 1) 'Child cache is represented by one parent game card'
 Check (-not [bool]((Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint) -and [bool]((Get-Item -LiteralPath $data).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Only the asset folder is mounted; executables remain on NTFS'
 Check ((Get-FileHash -LiteralPath (Join-Path $data 'assets.pak')).Hash -eq ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($original))).Replace('-','')) 'Mounted asset bytes match their original'
 $reject=$false;try{[void](Op @{operation='modeFull';id=$id})}catch{$reject=$_.Exception.Message -like '*libraryStopModeFirst*'}
 Check $reject 'Live scope changes are rejected'
 $state=Op @{operation='pause';id=$id};Check ((Item $state).phase -eq 'paused') 'Parent pause targets the asset worker'
 $state=Op @{operation='resume';id=$id};Check ((Item $state).phase -eq 'active') 'Parent resume targets the asset worker'
 $state=Op @{operation='clear';id=$id};Check ((Item $state).phase -eq 'active') 'Parent clear preserves the active asset mount'
 $state=Op @{operation='stop';id=$id}
 Check ((Item $state).phase -eq 'stopped' -and -not [bool]((Get-Item -LiteralPath $data).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Parent stop restores the ordinary asset directory'
 $state=Op @{operation='modeFull';id=$id}
 Check ((Item $state).cacheMode -eq 'full') 'Stopped game can switch to whole-folder mode'
 $state=Op @{operation='start';id=$id;app=$app}
 Check ((Item $state).phase -eq 'active' -and (Item $state).cacheMode -eq 'full' -and [bool]((Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Whole-folder mode still mounts the original game root'
 Check (@($state.apps|Where-Object {$_.path.StartsWith($source,[StringComparison]::OrdinalIgnoreCase)}).Count -eq 1) 'Inactive child remains hidden in whole-folder mode'
 [void](Op @{operation='stop';id=$id})
 # A renamed data directory must not silently expand to whole-game caching.
 Rename-Item -LiteralPath $data -NewName Paks-missing
 $reject=$false;try{[void](Op @{operation='modeData';id=$id});[void](Op @{operation='start';id=$id;app=$app})}catch{$reject=$_.Exception.Message -like '*libraryData*'}
 Check $reject 'Missing asset target fails without whole-folder fallback'
 $newData=Join-Path $source 'Other\Content\Paks'
 [void][IO.Directory]::CreateDirectory($newData)
 [IO.File]::WriteAllText((Join-Path $newData 'new.pak'),'New fixture location')
 $reject=$false;try{[void](Op @{operation='modeData';id=$id});[void](Op @{operation='start';id=$id;app=$app})}catch{$reject=$_.Exception.Message -like '*libraryDataChanged*'}
 Check $reject 'Changed target cannot reuse a different saved child folder'
 [IO.File]::Delete((Join-Path $newData 'new.pak'))
 [IO.Directory]::Delete($newData)
 Rename-Item -LiteralPath (Join-Path (Split-Path -Parent $data) 'Paks-missing') -NewName Paks
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message;throw}finally{
 try{
  $state=Call @('state');$row=Item $state
  if($row -and $row.phase -ne 'stopped'){[void](Op @{operation='stop';id=$id})}
  if($row){[void](Op @{operation='remove';id=$id})}
  $afterHash=(Get-FileHash -LiteralPath $store).Hash
  Check ($beforeHash -eq $afterHash) 'Existing profiles and configurations are restored byte for byte'
  if([IO.Path]::GetFullPath($source) -ne ('D:\SuperHDD-ScopeLab-'+$run) -or [IO.File]::ReadAllText((Join-Path $source 'owner.marker')) -ne $run){throw 'Fixture ownership mismatch'}
  if((Get-Item -LiteralPath $source).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Mounted fixture preserved'}
  if(@(Get-ChildItem -LiteralPath $source -Recurse -Force|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'Linked fixture preserved'}
  Remove-Item -LiteralPath $source -Recurse -Force
 }catch{$proof.passed=$false;$proof.error=[string]$proof.error+'; cleanup: '+$_.Exception.Message}
 $proof|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $evidence result.json)
 $proof|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $workspace 'artifacts\game-scope\integration-proof.json')
 $proof|ConvertTo-Json -Depth 8
}
if(-not $proof.passed){throw 'Game scope integration failed'}
