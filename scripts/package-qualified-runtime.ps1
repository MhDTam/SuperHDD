param([Parameter(Mandatory=$true)][string]$Driver,[Parameter(Mandatory=$true)][string]$Service,[Parameter(Mandatory=$true)][string]$QualificationReport)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$driverPath=(Resolve-Path -LiteralPath $Driver).Path
$servicePath=(Resolve-Path -LiteralPath $Service).Path
if([IO.Path]::GetFileName($driverPath) -ne 'SuperHDDCacheDriver.sys' -or [IO.Path]::GetFileName($servicePath) -ne 'SuperHDDService.exe'){throw 'Use the exact driver/service payload names.'}
# Validate the actual kernel policy, not only generic Authenticode or a lab cert.
$sdk='C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe'
if(-not(Test-Path -LiteralPath $sdk)){$sdk=Join-Path $root '.tools\sdk\bin\10.0.26100.0\x64\signtool.exe'}
if(-not(Test-Path -LiteralPath $sdk)){throw 'The Windows SDK kernel-policy verifier is required to assemble a qualified release.'}
& $sdk verify /kp /v $driverPath
if($LASTEXITCODE){throw 'Kernel-policy signature verification failed; no package was created.'}
$signature=Get-AuthenticodeSignature -LiteralPath $driverPath
if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Windows Hardware Compatibility Publisher'){throw 'A Microsoft hardware-publisher signature is required.'}
$driverHash=(Get-FileHash -LiteralPath $driverPath).Hash
$serviceHash=(Get-FileHash -LiteralPath $servicePath).Hash
$reportPath=(Resolve-Path -LiteralPath $QualificationReport).Path
$proof=Get-Content -LiteralPath $reportPath -Raw|ConvertFrom-Json
if($proof.schema -ne 1 -or $proof.protocolVersion -ne 5 -or $proof.driverSha256 -ne $driverHash){throw 'Qualification evidence is not bound to this exact signed driver.'}
foreach($field in @('physicalSourceQualified','dataCorrectnessPassed','sustainedLoadPassed','coldBootPassed','powerLossPassed','pnpRemovalPassed','lowMemoryPassed','installerRollbackPassed','uninstallWithActiveCachePassed','sourceAndCacheDistinct','microsoftPolicySignatureAccepted')){
    if($proof.$field -ne $true){throw ('Missing physical-release proof: '+$field)}
}
if(-not($proof.windowsClientBuilds|Where-Object {$_ -ge 19041 -and $_ -lt 22000}) -or -not($proof.windowsClientBuilds|Where-Object {$_ -ge 22000})){throw 'Bound Windows 10 and Windows 11 hardware runs are required.'}
$metadata=& $servicePath --release-info|ConvertFrom-Json
if($LASTEXITCODE -or $metadata.driverSha256 -ne $driverHash -or $metadata.protocolVersion -ne 5){throw 'Rebuild the service with SUPERHDD_QUALIFIED_DRIVER_SHA256 set to this qualified image hash.'}
$output=Join-Path $root ('artifacts\qualified-runtime-'+$driverHash.Substring(0,12).ToLowerInvariant())
if(Test-Path -LiteralPath $output){throw 'This package directory already exists; preserving it.'}
New-Item -ItemType Directory -Path $output|Out-Null
Copy-Item -LiteralPath $driverPath -Destination (Join-Path $output 'SuperHDDCacheDriver.sys')
Copy-Item -LiteralPath $servicePath -Destination (Join-Path $output 'SuperHDDService.exe')
Copy-Item -LiteralPath $reportPath -Destination (Join-Path $output 'qualification.json')
@{schema=1;protocolVersion=5;driverSha256=$driverHash;serviceSha256=$serviceHash;qualificationSha256=(Get-FileHash -LiteralPath $reportPath).Hash}|ConvertTo-Json|
    Set-Content -LiteralPath (Join-Path $output 'runtime-manifest.json') -Encoding utf8NoBOM
@{package=$output;desktopBuildEnvironment=@{SUPERHDD_QUALIFIED_DRIVER_SHA256=$driverHash;SUPERHDD_QUALIFIED_SERVICE_SHA256=$serviceHash}}|ConvertTo-Json -Depth 4
