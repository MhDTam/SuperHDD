param([int]$GameId=9588,[ValidateRange(15,600)][int]$Seconds=120)
$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$game=Get-Process -Id $GameId -ErrorAction Stop
if($game.ProcessName -ne 'eurotrucks2'){throw 'Unexpected game process'}
$started=$game.StartTime.ToUniversalTime().Ticks
$run=[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8)
$output=Join-Path $workspace ('artifacts\benchmarks\euro-resources-'+$run)
[IO.Directory]::CreateDirectory($output)|Out-Null
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;using System.Runtime.InteropServices;
public sealed class EuroCounters:IDisposable {
 [StructLayout(LayoutKind.Sequential)]struct Value {public uint Status;public double Number;}
 [DllImport("pdh.dll",CharSet=CharSet.Unicode)]static extern uint PdhOpenQueryW(string source,IntPtr user,out IntPtr query);
 [DllImport("pdh.dll",CharSet=CharSet.Unicode)]static extern uint PdhAddEnglishCounterW(IntPtr query,string path,IntPtr user,out IntPtr counter);
 [DllImport("pdh.dll")]static extern uint PdhCollectQueryData(IntPtr query);
 [DllImport("pdh.dll")]static extern uint PdhGetFormattedCounterValue(IntPtr counter,uint format,out uint type,out Value value);
 [DllImport("pdh.dll")]static extern uint PdhCloseQuery(IntPtr query);
 IntPtr query;Dictionary<string,IntPtr> counters=new Dictionary<string,IntPtr>();
 public EuroCounters(){uint e=PdhOpenQueryW(null,IntPtr.Zero,out query);if(e!=0)throw new Exception("PdhOpenQuery "+e);}
 public void Add(string key,string path){IntPtr counter;uint e=PdhAddEnglishCounterW(query,path,IntPtr.Zero,out counter);if(e!=0)throw new Exception("Counter "+path+": "+e);counters.Add(key,counter);}
 public Dictionary<string,double?> Read(){uint e=PdhCollectQueryData(query);if(e!=0)throw new Exception("Collect "+e);var result=new Dictionary<string,double?>();foreach(var entry in counters){uint type;Value value;uint status=PdhGetFormattedCounterValue(entry.Value,0x8200,out type,out value);result[entry.Key]=status==0&&(value.Status==0||value.Status==1)?(double?)value.Number:null;}return result;}
 public void Dispose(){if(query!=IntPtr.Zero){PdhCloseQuery(query);query=IntPtr.Zero;}}
}
'@
$paths=[ordered]@{
 processId='\Process(eurotrucks2)\ID Process'
 processCpuRawPercent='\Process(eurotrucks2)\% Processor Time'
 processPrivateWorkingSetBytes='\Process(eurotrucks2)\Working Set - Private'
 processPrivateBytes='\Process(eurotrucks2)\Private Bytes'
 processReadBytesPerSecond='\Process(eurotrucks2)\IO Read Bytes/sec'
 processWriteBytesPerSecond='\Process(eurotrucks2)\IO Write Bytes/sec'
 processPageFaultsPerSecond='\Process(eurotrucks2)\Page Faults/sec'
 sourceReadBytesPerSecond='\PhysicalDisk(2 D:)\Disk Read Bytes/sec'
 sourceWriteBytesPerSecond='\PhysicalDisk(2 D:)\Disk Write Bytes/sec'
 sourceReadSeconds='\PhysicalDisk(2 D:)\Avg. Disk sec/Read'
 sourceReadOperationsPerSecond='\PhysicalDisk(2 D:)\Disk Reads/sec'
 sourceQueueDepth='\PhysicalDisk(2 D:)\Current Disk Queue Length'
 sourceIdlePercent='\PhysicalDisk(2 D:)\% Idle Time'
 cacheDiskReadBytesPerSecond='\PhysicalDisk(3 M:)\Disk Read Bytes/sec'
 memoryAvailableMiB='\Memory\Available MBytes'
 systemPageReadsPerSecond='\Memory\Page Reads/sec'
 systemCpuPercent='\Processor(_Total)\% Processor Time'
}
$counters=[EuroCounters]::new();$writer=[IO.StreamWriter]::new((Join-Path $output 'samples.jsonl'),$false,[Text.UTF8Encoding]::new($false))
$meta=[ordered]@{runId=$run;gameId=$GameId;processStartTicks=$started;path=$game.Path;logicalProcessors=[Environment]::ProcessorCount;requestedSeconds=$Seconds;startedUtc=[DateTime]::UtcNow.ToString('o');scope='One-second Windows resource counters during gameplay; no frame times or exact per-file I/O attribution';cacheExpected='stopped';counterPaths=$paths;samples=0;error=$null}
$clock=[Diagnostics.Stopwatch]::StartNew()
try{
 foreach($key in $paths.Keys){$counters.Add($key,$paths[$key])}
 [void]$counters.Read()
 [IO.File]::WriteAllText((Join-Path $workspace 'artifacts\benchmarks\current-euro-resources.txt'),$output)
 'Resource recording started: '+$output
 while($clock.Elapsed.TotalSeconds -lt $Seconds -and -not(Test-Path -LiteralPath (Join-Path $output 'stop'))){
  Start-Sleep -Milliseconds 1000
  $game.Refresh();if($game.HasExited){break}
  if($game.StartTime.ToUniversalTime().Ticks -ne $started){throw 'Process identity changed'}
  $values=$counters.Read();if($values['processId'] -ne $GameId){throw 'Counter process identity changed'}
  $values['processCpuMachinePercent']=$values['processCpuRawPercent']/[Environment]::ProcessorCount
  $row=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');elapsedSeconds=$clock.Elapsed.TotalSeconds;values=$values}
  $writer.WriteLine(($row|ConvertTo-Json -Compress -Depth 5));$meta.samples++
  if($meta.samples%5 -eq 0){$writer.Flush()}
 }
}catch{$meta.error=$_.Exception.Message}finally{
 $writer.Dispose();$counters.Dispose();$meta.elapsedSeconds=$clock.Elapsed.TotalSeconds;$meta.endedUtc=[DateTime]::UtcNow.ToString('o')
 $meta|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $output 'metadata.json') -Encoding utf8
}
$meta|ConvertTo-Json -Depth 3
if($meta.error){throw $meta.error}
