param([string]$ProbeExe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\examples\download_probe.exe'))
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$runRoot = Join-Path $workspace ('artifacts\runtime\crash-' + [Guid]::NewGuid().ToString('N'))
$destination = 'E:\SuperHDD-Crash-' + [Guid]::NewGuid().ToString('N')
if (-not ([IO.Path]::GetFullPath($runRoot)).StartsWith($workspace + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected workspace root' }
New-Item -ItemType Directory -Path $runRoot,$destination | Out-Null
$jobId='abc-123'
$part=Join-Path $runRoot ($jobId + '.part')
$manifest=Join-Path $runRoot ($jobId + '.json')
$target=Join-Path $destination 'payload.bin'
$pending=Join-Path $destination ('.superhdd-' + $jobId + '.part')
$probe=(Resolve-Path -LiteralPath $ProbeExe).Path
$sourceStream=[IO.File]::Open($part,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
try { $buffer=New-Object byte[] 65536; for($i=0;$i -lt $buffer.Length;$i++){$buffer[$i]=[byte](($i*31)%251)};for($i=0;$i -lt 1024;$i++){$sourceStream.Write($buffer,0,$buffer.Length)};$sourceStream.Flush($true) } finally { $sourceStream.Dispose() }
$expected=(Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{id=$jobId;url='http://127.0.0.1:9/payload.bin';name='payload.bin';destination=$destination;bytes=67108864;total=67108864;speed=0;state='transferring';error=$null;etag='"fixture"';sha256=$expected;publishReady=$true;quota=134217728} | ConvertTo-Json | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM
$first=Start-Process -FilePath $probe -ArgumentList @(('"'+$runRoot+'"'),$jobId) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runRoot 'first.out') -RedirectStandardError (Join-Path $runRoot 'first.err')
$deadline=[DateTime]::UtcNow.AddSeconds(20)
while(-not (Test-Path -LiteralPath $pending)) { $first.Refresh();if($first.HasExited){throw ('Probe exited before crash boundary: '+(Get-Content -LiteralPath (Join-Path $runRoot 'first.err') -Raw))};if([DateTime]::UtcNow -gt $deadline){throw 'Copy boundary timed out'};Start-Sleep -Milliseconds 5 }
$pendingBytes=(Get-Item -LiteralPath $pending).Length
Stop-Process -Id $first.Id
$null=$first.WaitForExit(5000)
$second=Start-Process -FilePath $probe -ArgumentList @(('"'+$runRoot+'"'),$jobId) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runRoot 'second.out') -RedirectStandardError (Join-Path $runRoot 'second.err')
if(-not $second.WaitForExit(30000)){Stop-Process -Id $second.Id;throw 'Recovery timed out'}
if($second.ExitCode -ne 0){throw (Get-Content -LiteralPath (Join-Path $runRoot 'second.err') -Raw)}
$actual=(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
if($actual -ne $expected){throw 'Published payload hash mismatch'}
$recovered=Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
if($recovered.state -ne 'completed' -or (Test-Path -LiteralPath $part) -or (Test-Path -LiteralPath $pending)){throw 'Recovery/cleanup did not complete'}
$report=[pscustomobject]@{at=[DateTime]::UtcNow.ToString('o');test='Actual worker process terminated during destination copy, then restarted';payloadBytes=67108864;pendingBytesAtTermination=$pendingBytes;firstPid=$first.Id;recovered=$true;sha256Matches=$true;temporaryFilesReclaimed=$true}
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $workspace 'artifacts\runtime\download-crash-result.json') -Encoding utf8NoBOM
# Delete only the exact disposable published payload; never recurse over E:.
if(-not ([IO.Path]::GetFullPath($target)).StartsWith($destination+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected cleanup target'}
Remove-Item -LiteralPath $target
if(-not(Get-ChildItem -LiteralPath $destination -Force | Select-Object -First 1)){Remove-Item -LiteralPath $destination}
$report | ConvertTo-Json
