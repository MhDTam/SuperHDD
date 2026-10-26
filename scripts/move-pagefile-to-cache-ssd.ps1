# Prepared for explicit approval. Default invocation only records a plan.
# Windows applies the page-file location at startup; this script never reboots.
param([switch]$Apply)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$key='HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
$computer=Get-CimInstance Win32_ComputerSystem
$usage=@(Get-CimInstance Win32_PageFileUsage)
$paging=[string[]](Get-ItemPropertyValue -LiteralPath $key -Name PagingFiles)
$destination=Get-Partition -DriveLetter M|Get-Disk
if($usage.Count -ne 1 -or $usage[0].Name -ne 'D:\pagefile.sys' -or $destination.UniqueId -ne 'eui.002538B631A16109' -or $destination.IsBoot -or $destination.IsSystem -or $destination.IsOffline){throw 'Expected current D page file and reviewed online M SSD are required.'}
$free=(Get-PSDrive M).Free
$reserve=100GB+24999952384+[long]$computer.TotalPhysicalMemory
if($free -lt $reserve){throw 'M must retain its cache budget, page-file headroom and SSD reserve.'}
$plan=[ordered]@{at=[DateTime]::UtcNow.ToString('o');applied=$false;restartRequired=$true;automaticRestart=$false;currentPageFile=$usage[0].Name;currentAllocatedMiB=$usage[0].AllocatedBaseSize;originalAutomaticManagedPagefile=[bool]$computer.AutomaticManagedPagefile;originalPagingFiles=$paging;proposedPagingFiles=@('M:\pagefile.sys 0 0');destinationFreeBytes=$free;destinationSizeManagedByWindows=$true;cacheBudgetBytes=24999952384}
$record=Join-Path $root 'artifacts\diagnostics\pagefile-relocation-plan.json'
if(-not $Apply){$plan|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $record -Encoding utf8NoBOM;return}
$principal=[Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator consent is required to apply the approved page-file change.'}
$backup=Join-Path $root ('artifacts\diagnostics\pagefile-backup-'+[Guid]::NewGuid().ToString('N')+'.json')
$plan|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $backup -Encoding utf8NoBOM
try{
    $computer|Set-CimInstance -Property @{AutomaticManagedPagefile=$false}
    Set-ItemProperty -LiteralPath $key -Name PagingFiles -Value ([string[]]@('M:\pagefile.sys 0 0'))
    $after=[string[]](Get-ItemPropertyValue -LiteralPath $key -Name PagingFiles)
    if($after.Count -ne 1 -or $after[0] -ne 'M:\pagefile.sys 0 0' -or (Get-CimInstance Win32_ComputerSystem).AutomaticManagedPagefile){throw 'Page-file location verification failed.'}
    $plan.applied=$true
}catch{
    $computer|Set-CimInstance -Property @{AutomaticManagedPagefile=[bool]$plan.originalAutomaticManagedPagefile}
    Set-ItemProperty -LiteralPath $key -Name PagingFiles -Value $paging
    throw
}finally{$plan|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
