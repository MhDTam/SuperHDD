param([Parameter(Mandatory=$true)][string]$ReleaseServiceExe)
$ErrorActionPreference='Stop'
if(-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Run explicitly as Administrator.'}
$source=(Resolve-Path -LiteralPath $ReleaseServiceExe).Path
if([IO.Path]::GetFileName($source) -ne 'SuperHDDService.exe'){throw 'Select the built SuperHDDService.exe.'}
if(Get-Service -Name SuperHDDService -ErrorAction SilentlyContinue){throw 'Service already exists; review its configuration before updating.'}
$destination=Join-Path $env:ProgramFiles 'Super HDD'
if(Test-Path -LiteralPath $destination){throw 'Installation directory already exists; review it before installing.'}
New-Item -ItemType Directory -Path $destination | Out-Null
$target=Join-Path $destination 'SuperHDDService.exe'
Copy-Item -LiteralPath $source -Destination $target -ErrorAction Stop
# Inherit Program Files ACLs; do not register a service executable from a user-writable tree.
& icacls.exe $destination /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
if($LASTEXITCODE){throw 'Failed to apply protected installation permissions.'}
New-Service -Name SuperHDDService -DisplayName 'Super HDD Service' -BinaryPathName ('"'+$target+'"') -StartupType Automatic -Description 'Read-only local storage discovery and cache-driver diagnostics.' | Out-Null
Start-Service -Name SuperHDDService
Get-Service -Name SuperHDDService
