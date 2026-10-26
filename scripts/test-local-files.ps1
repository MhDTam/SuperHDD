# Bounded current-machine test: only new ordinary files, never volume formatting.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$run=[Guid]::NewGuid().ToString('N')
$output=Join-Path $root ('artifacts\runtime\local-files-'+$run)
$binary=Join-Path $root 'target\release\examples\hardware_bench.exe'
if(-not(Test-Path -LiteralPath $binary)){throw 'Build the current hardware_bench example first.'}
$lock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$owned=@();$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;scope='Current-machine disposable ordinary-file cache API checks; not transparent host acceleration';passed=$false;temporarySourceBytes=2GB;temporarySsdBudgetBytes=256MB;dedicatedRamCacheBytes=0;hostSecurityChanged=$false;hostDriverInstalled=$false;drives=@();cleanup=@();error=$null}
function Identity([string]$Letter){
    $partition=Get-Partition -DriveLetter $Letter
    $disk=$partition|Get-Disk
    $volume=$partition|Get-Volume
    if($volume.FileSystem -ne 'NTFS' -or $volume.HealthStatus -ne 'Healthy' -or $disk.IsOffline -or $disk.IsReadOnly -or $volume.SizeRemaining -lt 5GB){throw ('Unsuitable file-test volume: '+$Letter)}
    @{letter=$Letter;volume=[string]$volume.UniqueId;diskId=[string]$disk.UniqueId;diskNumber=$disk.Number;model=$disk.FriendlyName}
}
function Same-Identity($Before){
    $now=Identity $Before.letter
    if($now.volume -ne $Before.volume -or $now.diskId -ne $Before.diskId){throw 'Drive identity changed; preserving owned test files.'}
}
try{
    New-Item -ItemType Directory -Path $output|Out-Null
    $ssd=Identity 'M'
    $result.binarySha256=(Get-FileHash -LiteralPath $binary).Hash
    $result.sourceHashes=@('crates\cache-core\src\lib.rs','crates\cache-core\examples\hardware_bench.rs','scripts\test-local-files.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
    $result.os=Get-CimInstance Win32_OperatingSystem|Select-Object Caption,Version,OSArchitecture
    foreach($letter in @('D','E')){
        $source=Identity $letter
        if($source.diskId -eq $ssd.diskId){throw 'Source and SSD share a physical disk.'}
        $sourceDirectory=$letter+':\SuperHDD-LocalCheck-'+$run
        $cacheDirectory=Join-Path $output ('cache-'+$letter)
        if(Test-Path -LiteralPath $sourceDirectory){throw 'Source test directory already exists.'}
        # The benchmark creates both directories with create_dir, and each payload
        # with create_new. Record only these exact new paths before invocation.
        $entry=@{source=$source;ssd=$ssd;sourceDirectory=$sourceDirectory;cacheDirectory=$cacheDirectory}
        $owned+=$entry
        $result|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $output 'state.json') -Encoding utf8NoBOM
        & $binary $sourceDirectory $cacheDirectory 134217728 > (Join-Path $output ($letter+'.log')) 2>&1
        if($LASTEXITCODE){throw ('Local file-cache check failed for '+$letter)}
        $report=Get-Content -LiteralPath (Join-Path $cacheDirectory 'results.json') -Raw|ConvertFrom-Json
        if($report.source -ne (Join-Path $sourceDirectory 'superhdd-disposable-source.bin') -or $report.backing -ne (Join-Path $cacheDirectory 'ssd-cache.bin') -or $report.capacityBytes -ne 128MB -or @($report.checks.PSObject.Properties|Where-Object Value -ne $true).Count){throw 'Local benchmark result did not pass all checks.'}
        Copy-Item -LiteralPath (Join-Path $cacheDirectory 'results.json') -Destination (Join-Path $output ($letter+'-results.json'))
        $result.drives+=@{source=$source;ssd=$ssd;checks=$report.checks;report=($letter+'-results.json')}
        Same-Identity $source
        Same-Identity $ssd
    }
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    foreach($entry in $owned){
        try{
            Same-Identity $entry.source
            Same-Identity $entry.ssd
            foreach($directory in @($entry.sourceDirectory,$entry.cacheDirectory)){
                $absolute=[IO.Path]::GetFullPath($directory)
                $isSource=$absolute -eq [IO.Path]::GetFullPath($entry.sourceDirectory)
                $expected=if($isSource){$entry.source.letter+':\SuperHDD-LocalCheck-'+$run}else{Join-Path $output ('cache-'+$entry.source.letter)}
                if($absolute -ne [IO.Path]::GetFullPath($expected)){throw 'Cleanup escaped the exact new test directory.'}
                if(-not(Test-Path -LiteralPath $absolute)){continue}
                $directoryInfo=Get-Item -LiteralPath $absolute
                if($directoryInfo.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Test directory became a reparse point; preserved.'}
                $names=if($isSource){@('superhdd-disposable-source.bin')}else{@('ssd-cache.bin','results.json')}
                $children=@(Get-ChildItem -LiteralPath $absolute -Force)
                if(@($children|Where-Object {$_.PSIsContainer -or $_.Name -notin $names -or ($_.Attributes -band [IO.FileAttributes]::ReparsePoint)}).Count){throw 'Unexpected test-directory contents; preserved.'}
                foreach($file in $children){Remove-Item -LiteralPath $file.FullName}
                Remove-Item -LiteralPath $absolute
            }
            $result.cleanup+=@{letter=$entry.source.letter;sourceRemoved=(-not(Test-Path -LiteralPath $entry.sourceDirectory));cacheRemoved=(-not(Test-Path -LiteralPath $entry.cacheDirectory))}
        }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
    }
    if($result.cleanup.Count -ne 2 -or @($result.cleanup|Where-Object {-not $_.sourceRemoved -or -not $_.cacheRemoved}).Count){$result.passed=$false}
    if(Test-Path -LiteralPath $output){$result|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $output 'result.json') -Encoding utf8NoBOM}
    $lock.Dispose()
}
$result|ConvertTo-Json -Depth 5
if(-not $result.passed){throw 'Current-machine file checks did not pass; inspect the owned run report.'}
