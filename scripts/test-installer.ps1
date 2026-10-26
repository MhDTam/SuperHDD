param(
    [string]$Installer = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\bundle\nsis\Super HDD_0.1.0_x64-setup.exe'),
    [string]$ReportName = ('installer-test-'+[DateTime]::UtcNow.ToString('yyyyMMdd')+'.json')
)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if($ReportName -notmatch '^installer-test-[0-9]{8}\.json$'){throw 'Invalid report filename'}
$installDir = Join-Path $workspace ('artifacts\runtime\installer-' + [Guid]::NewGuid().ToString('N'))
$uninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Super HDD'
$productKey = 'Software\superhdd\Super HDD'
$manufacturerKey = 'Software\superhdd'
$views = @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)
$manufacturerExisted = @{}
foreach($view in $views){
    $hive=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,$view)
    try {
        foreach($key in @($uninstallKey,$productKey)){
            $existing=$hive.OpenSubKey($key)
            if($existing){$existing.Dispose();throw "Existing installation state: $key ($view). Test will not replace it."}
        }
        $manufacturer=$hive.OpenSubKey($manufacturerKey)
        $manufacturerExisted[$view.ToString()]=($null -ne $manufacturer)
        if($manufacturer){$manufacturer.Dispose()}
        $run=$hive.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run')
        try {if($run -and $null -ne $run.GetValue('Super HDD')){throw 'Existing startup entry; test will not change it.'}}finally{if($run){$run.Dispose()}}
    } finally {$hive.Dispose()}
}
if(Get-CimInstance Win32_Process -Filter "Name='SuperHDD.exe'"){throw 'Close the project application before the installer test.'}
if(-not $installDir.StartsWith($workspace+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected install path'}
$resolvedInstaller=(Resolve-Path -LiteralPath $Installer).Path
$install=Start-Process -FilePath $resolvedInstaller -ArgumentList ('/S /NS /D='+$installDir) -WindowStyle Hidden -PassThru
if(-not $install.WaitForExit(30000)){throw 'Installer timed out'}
if($install.ExitCode -ne 0){throw "Installer failed: $($install.ExitCode)"}
$installedExe=Join-Path $installDir 'SuperHDD.exe'
$uninstaller=Join-Path $installDir 'uninstall.exe'
if(-not(Test-Path -LiteralPath $installedExe) -or -not(Test-Path -LiteralPath $uninstaller)){throw 'Installed binaries are missing'}
if((Get-FileHash -LiteralPath $installedExe).Hash -ne (Get-FileHash -LiteralPath (Join-Path $workspace 'target\release\SuperHDD.exe')).Hash){throw 'Installed executable differs from release build'}
$profile=Join-Path $env:LOCALAPPDATA 'com.superhdd.desktop'
$profilePresent=Test-Path -LiteralPath $profile
$process=Start-Process -FilePath $installedExe -WindowStyle Hidden -PassThru
try {
    $null=$process.WaitForInputIdle(15000)
    Start-Sleep -Seconds 2
    $process.Refresh()
    if($process.HasExited -or $process.MainWindowTitle -ne 'Super HDD'){throw 'Installed application failed to open its window'}
} finally {
    $process.Refresh()
    if(-not $process.HasExited){Stop-Process -Id $process.Id;$null=$process.WaitForExit(5000)}
}
$profileMarker=$null
if($profilePresent){
    $profileMarker=Join-Path $profile ('installer-proof-'+[Guid]::NewGuid().ToString('N')+'.tmp')
    [IO.File]::WriteAllText($profileMarker,'Super HDD isolated uninstall preservation fixture')
    $markerHash=(Get-FileHash -LiteralPath $profileMarker).Hash
}
# NSIS consumes the final _?= path without quotes; silent mode leaves app-data deletion unchecked.
if(-not ([IO.Path]::GetFullPath($installDir)).StartsWith($workspace+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected uninstall path'}
$uninstall=Start-Process -FilePath $uninstaller -ArgumentList ('/S _?='+$installDir) -WindowStyle Hidden -PassThru
if(-not $uninstall.WaitForExit(30000)){throw 'Uninstaller timed out'}
if($uninstall.ExitCode -ne 0){throw "Uninstaller failed: $($uninstall.ExitCode)"}
if(Test-Path -LiteralPath $installedExe){throw 'Uninstaller left the application executable'}
if($profilePresent -and -not(Test-Path -LiteralPath $profile)){throw 'Silent uninstallation unexpectedly removed the user profile'}
if($profileMarker){
    if(-not(Test-Path -LiteralPath $profileMarker) -or (Get-FileHash -LiteralPath $profileMarker).Hash -ne $markerHash){throw 'Uninstallation changed the app-data preservation fixture'}
    Remove-Item -LiteralPath $profileMarker
}
foreach($view in $views){
    $hive=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,$view)
    try {
        $uninstallState=$hive.OpenSubKey($uninstallKey)
        if($uninstallState){$uninstallState.Dispose();throw 'Uninstaller left its program registration'}
        $product=$hive.OpenSubKey($productKey)
        if($product){
            $recorded=$product.GetValue('');$children=$product.GetSubKeyNames();$product.Dispose()
            if($recorded -ne $installDir -or $children.Count -ne 0){throw 'Unexpected product state; cleanup stopped'}
            $hive.DeleteSubKey($productKey)
        }
        if(-not $manufacturerExisted[$view.ToString()]){
            $manufacturer=$hive.OpenSubKey($manufacturerKey)
            if($manufacturer){
                $empty=$manufacturer.ValueCount -eq 0 -and $manufacturer.SubKeyCount -eq 0;$manufacturer.Dispose()
                if($empty){$hive.DeleteSubKey($manufacturerKey)}
            }
        }
    } finally {$hive.Dispose()}
}
# Running an uninstaller in-place may leave its own executable. Remove only that known test file.
if(Test-Path -LiteralPath $uninstaller){Remove-Item -LiteralPath $uninstaller}
if(Test-Path -LiteralPath $installDir){
    if(Get-ChildItem -LiteralPath $installDir -Force | Select-Object -First 1){throw 'Unexpected file in installer test directory'}
    Remove-Item -LiteralPath $installDir
}
$report=[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');scope='Current-user desktop-only NSIS; isolated workspace destination; no driver/service install';installed=$true;executableHashMatches=$true;installedWindowOpened=$true;uninstalled=$true;existingUserProfilePreserved=$true;testRegistrationCleaned=$true;testDirectoryRemoved=$true}
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $workspace ('artifacts\runtime\'+$ReportName)) -Encoding utf8NoBOM
$report | ConvertTo-Json
