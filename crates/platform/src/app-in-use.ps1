param([string]$Source,[string]$Backing='')
$ErrorActionPreference='Stop'
$env:PSModulePath=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
# An asset-only cache has no executables beneath Source. Include registered game
# ancestors so mounting/stopping a data subtree cannot evade the running-game check.
$processRoots=@($Source)
if($Backing){$processRoots+=$Backing}
$store=Join-Path $PSScriptRoot '..\..\..\artifacts\library-runtime\library.json'
if(Test-Path -LiteralPath $store){
 $profiles=(Get-Content -LiteralPath $store -Raw|ConvertFrom-Json).apps
 foreach($app in $profiles){
  if($app.kind -eq 'game' -and $app.path -and $Source.StartsWith($app.path.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){$processRoots+=$app.path}
 }
}
$busy=@(Get-CimInstance Win32_Process|Where-Object {
 $executable=$_.ExecutablePath
 if($executable){foreach($root in $processRoots){if($executable.StartsWith($root.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){return $true}}}
 $false
})
if($busy.Count){exit 2}
exit 0
