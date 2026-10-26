$ErrorActionPreference='Stop'
$lab=Join-Path (Split-Path -Parent $PSScriptRoot) 'artifacts\runtime\kernel-lab'
$path=Join-Path $lab 'os.vhdx'
$report=[ordered]@{at=[DateTime]::UtcNow.ToString('o');checks=@();error=$null}
$mounted=$false
try{
    $vhd=Mount-VHD -Path $path -PassThru;$mounted=$true;$disk=$vhd|Get-Disk
    if((Get-VHD -DiskNumber $disk.Number).Path -ne $path -or $disk.IsBoot -or $disk.IsSystem){throw 'Unexpected disposable VHD identity.'}
    $partition=Get-Partition -DiskNumber $disk.Number | Where-Object {$_.Type -eq 'IFS'} | Select-Object -First 1
    $report.disk=$disk | Select-Object Number,HealthStatus,OperationalStatus,IsOffline,IsReadOnly,BusType
    $volume=$partition|Get-Volume;$report.volume=$volume|Select-Object DriveLetter,HealthStatus,Size,SizeRemaining
    $file=Join-Path ($volume.DriveLetter+':\') 'superhdd-owned-io-test.bin'
    $stream=[IO.File]::Open($file,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{ $buffer=[byte[]]::new(1MB);[Random]::new(37).NextBytes($buffer);for($i=0;$i -lt 512;$i++){$stream.Write($buffer,0,$buffer.Length)};$stream.Flush($true) }finally{$stream.Dispose()}
    $report.checks+=@{operation='512 MiB write and flush';passed=$true}
    Remove-Item -LiteralPath $file
}catch{$report.error=$_.Exception.Message}finally{
    if($mounted){Dismount-VHD -Path $path -ErrorAction Continue}
    $report|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $lab 'vhd-io-inspection.json') -Encoding utf8NoBOM
}
