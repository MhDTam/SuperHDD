param([Parameter(Mandatory=$true)][string]$Package)
$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Runtime installation requires an explicit Windows administrator operation.'}
$packageRoot=(Resolve-Path -LiteralPath $Package).Path
$manifest=Get-Content -LiteralPath (Join-Path $packageRoot 'runtime-manifest.json') -Raw|ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.protocolVersion -ne 5){throw 'Unknown runtime package.'}
foreach($entry in @(@{file='SuperHDDCacheDriver.sys';hash=$manifest.driverSha256},@{file='SuperHDDService.exe';hash=$manifest.serviceSha256},@{file='qualification.json';hash=$manifest.qualificationSha256})){
    if($entry.hash -notmatch '^[A-Fa-f0-9]{64}$' -or (Get-FileHash -LiteralPath (Join-Path $packageRoot $entry.file)).Hash -ne $entry.hash){throw 'Package payload hash mismatch.'}
}
$signature=Get-AuthenticodeSignature -LiteralPath (Join-Path $packageRoot 'SuperHDDCacheDriver.sys')
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Windows Hardware Compatibility Publisher'){throw 'The package does not contain a valid Microsoft hardware-publisher-signed driver.'}
$proof=Get-Content -LiteralPath (Join-Path $packageRoot 'qualification.json') -Raw|ConvertFrom-Json
if($proof.schema -ne 1 -or $proof.protocolVersion -ne 5 -or $proof.driverSha256 -ne $manifest.driverSha256 -or $proof.physicalSourceQualified -ne $true){throw 'Hardware qualification does not match this driver.'}
foreach($field in @('dataCorrectnessPassed','sustainedLoadPassed','coldBootPassed','powerLossPassed','pnpRemovalPassed','lowMemoryPassed','installerRollbackPassed','uninstallWithActiveCachePassed','sourceAndCacheDistinct','microsoftPolicySignatureAccepted')){
    if($proof.$field -ne $true){throw ('Missing hardware release proof: '+$field)}
}
if(-not($proof.windowsClientBuilds|Where-Object {$_ -ge 19041 -and $_ -lt 22000}) -or -not($proof.windowsClientBuilds|Where-Object {$_ -ge 22000})){throw 'Bound Windows 10 and Windows 11 hardware runs are required.'}
if(Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"){throw 'An existing service must be reviewed before installing another runtime.'}
if(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheDriver'"){throw 'A driver with this name already exists.'}
$destination=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Super HDD Runtime'
if(Test-Path -LiteralPath $destination){throw 'The runtime destination already exists; preserving it.'}
$created=@()
try{
    New-Item -ItemType Directory -Path $destination|Out-Null
    & icacls.exe $destination /inheritance:r /setowner '*S-1-5-32-544' /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX'|Out-Null
    if($LASTEXITCODE){throw 'Cannot protect runtime installation.'}
    foreach($name in @('SuperHDDCacheDriver.sys','SuperHDDService.exe','runtime-manifest.json')){
        $target=Join-Path $destination $name
        Copy-Item -LiteralPath (Join-Path $packageRoot $name) -Destination $target
        $created+=$target
        & icacls.exe $target /setowner '*S-1-5-32-544'|Out-Null
        if($LASTEXITCODE){throw 'Cannot set the trusted payload owner.'}
    }
    # Services and source enrollment are created only when the user starts a
    # validated cache plan through the installed, hash-bound UAC helper.
    [pscustomobject]@{installed=$true;runtime=$destination;sourceEnrollmentPerformed=$false;cacheActive=$false}
}catch{
    $fullRoot=[IO.Path]::GetFullPath($destination)
    foreach($file in $created){$absolute=[IO.Path]::GetFullPath($file);if([IO.Path]::GetDirectoryName($absolute) -ne $fullRoot){throw 'Rollback path escaped the exact new runtime directory.'};Remove-Item -LiteralPath $absolute}
    if((Test-Path -LiteralPath $destination) -and @(Get-ChildItem -LiteralPath $destination -Force).Count -eq 0){Remove-Item -LiteralPath $destination}
    throw
}
