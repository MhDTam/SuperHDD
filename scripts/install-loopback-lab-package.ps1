# Install only this development package into a fresh protected Program Files
# directory. Does not create a service, offline D or enable physical caching.
param([switch]$UpdateOwned,[switch]$EnablePhysical)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$destination=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'Super HDD Local Runtime'
if(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json'){throw 'Stop and remove the prepared runtime owner before changing its executable package.'}
foreach($name in @('SuperHDDLoopbackService','SuperHDDLoopbackLab')){if(Get-Service -Name $name -ErrorAction SilentlyContinue){throw 'Existing native service must be stopped and removed before replacing its package.'}}
$sources=[ordered]@{
    'SuperHDDService.exe'=(Join-Path $root 'target\loopback-service\release\SuperHDDService.exe')
    'SuperHDDCacheWorker.exe'=(Join-Path $root 'artifacts\loopback-runtime\SuperHDDCacheWorker.exe')
    'superhdd_loopback_runtime.dll'=(Join-Path $root 'artifacts\loopback-runtime\superhdd_loopback_runtime.dll')
    'ISCSI.dll'=(Join-Path $root 'artifacts\loopback-runtime\ISCSI.dll')
    'DiskAccessLibrary.dll'=(Join-Path $root 'artifacts\loopback-runtime\DiskAccessLibrary.dll')
}
$pins=@();$hashes=[ordered]@{}
try{
    foreach($name in $sources.Keys){$file=[IO.File]::Open($sources[$name],[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read);$pins+=,$file;$hashes[$name]=(Get-FileHash -InputStream $file -Algorithm SHA256).Hash;$file.Position=0}
    if($EnablePhysical){
        $proof=Get-Content -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-lab.json') -Raw|ConvertFrom-Json
        if(-not $proof.passed -or -not $proof.scmAutomaticallyRecoveredWorker -or -not $proof.scmExistingWorkerAdopted -or -not $proof.scmBusyStopRefused -or -not $proof.scmStoppedAndRemoved -or -not $proof.publicReadOnlyStatusVerified -or -not $proof.pausePersistedAfterWorkerRestart -or -not $proof.sourceRemoved -or -not $proof.ownerManifestRemoved -or -not $proof.bootUnchanged -or -not $proof.parentsUnchanged){throw 'A complete current owned-VHDX lifecycle proof is required before physical setup.'}
        foreach($name in $sources.Keys){if($proof.packageBinaries.$name -ne $hashes[$name]){throw 'Candidate differs from the tested package; physical setup is not enabled.'}}
    }
    if(Test-Path -LiteralPath $destination){
        if(-not $UpdateOwned){throw 'Existing package directory is preserved; explicit package update is required.'}
        $verified=& $sources['SuperHDDService.exe'] --verify-loopback-package
        if($LASTEXITCODE -or -not ($verified|ConvertFrom-Json).verified){throw 'A verified previous native package is required.'}
        if(-not ($verified|ConvertFrom-Json).labOnly){
            if(-not $EnablePhysical){throw 'An existing physical package is preserved.'}
            $installed=Get-Content -LiteralPath (Join-Path $destination 'native-package.json') -Raw|ConvertFrom-Json
            foreach($name in $sources.Keys){if($installed.files.$name -ne $hashes[$name]){throw 'An existing physical package differs from the qualified candidate; preserved.'}}
            # Exact current physical package already installed after a prior
            # source-busy refusal. Reuse it without replacing any binary.
            return
        }
        $children=@(Get-ChildItem -LiteralPath $destination -Force)
        $expected=@($sources.Keys)+@('native-package.json')
        if($children.Count -ne $expected.Count -or @($children|Where-Object {$_.PSIsContainer -or $_.Name -notin $expected}).Count){throw 'Unexpected package contents are preserved.'}
        # Exact fixed root and a closed file list, never recursive deletion.
        $resolved=[IO.Path]::GetFullPath($destination)
        $wanted=[IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'Super HDD Local Runtime'))
        if($resolved -ne $wanted){throw 'Package update escaped its fixed root.'}
        foreach($name in $expected){Remove-Item -LiteralPath (Join-Path $resolved $name) -ErrorAction Stop}
        Remove-Item -LiteralPath $resolved -ErrorAction Stop
    }
    $null=New-Item -ItemType Directory -Path $destination
    $directoryAcl=[Security.AccessControl.DirectorySecurity]::new()
    $directoryAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)')
    Set-Acl -LiteralPath $destination -AclObject $directoryAcl
    $index=0
    foreach($name in $sources.Keys){
        $path=Join-Path $destination $name
        $output=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
        try{$pins[$index].CopyTo($output);$output.Flush($true)}finally{$output.Dispose()};$index++
        $acl=[Security.AccessControl.FileSecurity]::new();$acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)');Set-Acl -LiteralPath $path -AclObject $acl
        if((Get-FileHash -LiteralPath $path).Hash -ne $hashes[$name]){throw 'Installed package differs from its pinned input.'}
    }
    $manifestPath=Join-Path $destination 'native-package.json'
    [ordered]@{schema=1;protocolVersion=6;labOnly=(-not $EnablePhysical);files=$hashes}|ConvertTo-Json -Depth 3|Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    $acl=[Security.AccessControl.FileSecurity]::new();$acl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;BU)');Set-Acl -LiteralPath $manifestPath -AclObject $acl
}finally{foreach($file in $pins){$file.Dispose()}}
