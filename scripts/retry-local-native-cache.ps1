# Retry the reviewed D/M setup after temporarily releasing Windows link tracking.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Start-Transcript -LiteralPath (Join-Path $root 'artifacts\physical-retry-run.log') -Force|Out-Null
try{
& (Join-Path $PSScriptRoot 'inspect-d-volume-users.ps1')
$previous=Get-Service TrkWks
if($previous.Status -notin @('Running','Stopped')){throw 'Link tracking is transitioning; preserved.'}
$evidence=Get-Content -LiteralPath (Join-Path $root 'artifacts\diagnostics\d-volume-users.json') -Raw|ConvertFrom-Json
$owners=@($evidence.processes)
if($owners.Count -ne 1 -or $owners[0].process -ne 'svchost.exe'){throw 'Expected one identified Windows source holder.'}
$services=@(Get-CimInstance Win32_Service -Filter ('ProcessId='+$owners[0].pid))
if($services.Count -ne 1 -or $services[0].Name -ne 'TrkWks'){throw 'Source holder changed; no service stopped.'}
$stopped=$false
try{
    if($previous.Status -eq 'Running'){Stop-Service TrkWks;$stopped=$true;(Get-Service TrkWks).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(15))}
    & (Join-Path $PSScriptRoot 'enable-local-native-cache.ps1')
}finally{
    if($stopped){Start-Service TrkWks}
    [ordered]@{at=[DateTime]::UtcNow.ToString('o');name='TrkWks';previous=$previous.Status.ToString();restored=((Get-Service TrkWks).Status -eq $previous.Status)}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $root 'artifacts\diagnostics\link-tracking-restoration.json') -Encoding utf8NoBOM
}
}finally{Stop-Transcript|Out-Null}
