$ErrorActionPreference='Stop'
$workspace=(Get-Location).Path
$root=Join-Path $workspace ('artifacts\library-icons\fixtures-'+[Guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
. ([scriptblock]::Create([IO.File]::ReadAllText((Join-Path $workspace 'crates\platform\src\library-icons.ps1'))))
$checks=New-Object 'System.Collections.Generic.List[string]'
function Check([bool]$ok,[string]$name){if(-not $ok){throw $name};$checks.Add($name)}
function Write-Icon([string]$directory){
 [void][IO.Directory]::CreateDirectory($directory)
 $stream=[IO.File]::Create((Join-Path $directory 'game.ico'))
 try{[Drawing.SystemIcons]::Information.Save($stream)}finally{$stream.Dispose()}
}
$nested=Join-Path $root 'Nested\Binaries\Win64'
Write-Icon $nested
$uri=Get-LibraryIcon (Join-Path $root 'Nested')
Check ($uri -like 'data:image/png;base64,*') 'Nested game resources are found without a root executable'
$bytes=[Convert]::FromBase64String($uri.Substring($uri.IndexOf(',')+1))
$stream=New-Object IO.MemoryStream(,$bytes)
try{$image=[Drawing.Image]::FromStream($stream);try{Check ($image.Width -eq 48 -and $image.Height -eq 48) 'Output is a valid 48px PNG'}finally{$image.Dispose()}}finally{$stream.Dispose()}
$quoted=Join-Path $root 'quoted, folder'
Write-Icon $quoted
Check ((ConvertTo-LibraryIcon ('"'+(Join-Path $quoted 'game.ico')+'",0')) -eq $uri) 'Quoted resource paths with commas and icon index are supported'
$empty=Join-Path $root 'No resources'
[void][IO.Directory]::CreateDirectory($empty)
[IO.File]::WriteAllText((Join-Path $empty 'broken.exe'),'not a PE image')
Check (-not (Get-LibraryIcon $empty)) 'Malformed executables do not produce generic shell icons or fail discovery'
$installer=Join-Path $root 'Installer only'
[void][IO.Directory]::CreateDirectory($installer)
Copy-Item -LiteralPath (Join-Path $PSHOME 'powershell.exe') -Destination (Join-Path $installer 'setup.exe')
Check (-not (Get-LibraryIcon $installer)) 'Installer resources are excluded from game fallback'
Check ((Get-LibraryIcon (Join-Path $root 'Nested') @((Join-Path $installer 'setup.exe'))) -eq $uri) 'Installer DisplayIcon does not override the real game resource'
Check ((Get-LibraryIcon $empty @((Join-Path $installer 'setup.exe')) $false) -like 'data:image/png;base64,*') 'Windows app metadata can retain its registered installer icon'
Check ((Get-LibraryIcon $empty @('Z:\missing.ico',(Join-Path $nested 'game.ico'))) -eq $uri) 'Missing preferred resource falls through to the next valid resource'
$beyond=Join-Path $root 'Too deep\a\b\c\d\e\f'
Write-Icon $beyond
Check (-not (Get-LibraryIcon (Join-Path $root 'Too deep'))) 'Search depth remains bounded'
$junctionRoot=Join-Path $root 'Junction root'
[void][IO.Directory]::CreateDirectory($junctionRoot)
New-Item -ItemType Junction -Path (Join-Path $junctionRoot 'linked') -Target $nested | Out-Null
Check (-not (Get-LibraryIcon $junctionRoot)) 'Search does not follow child directory junctions'
$profiles=@(foreach($i in 1..85){$path=Join-Path $root ('Manual '+$i);Write-Icon $path;@{name='Icon fixture '+$i;path=$path}})
$oldProfiles=$env:SUPERHDD_LIBRARY_PROFILES
try{
 $env:SUPERHDD_LIBRARY_PROFILES=ConvertTo-Json -InputObject $profiles -Compress
 $script=[IO.File]::ReadAllText((Join-Path $workspace 'crates\platform\src\library-icons.ps1'))+"`n"+[IO.File]::ReadAllText((Join-Path $workspace 'crates\platform\src\installed_apps.ps1'))
 $watch=[Diagnostics.Stopwatch]::StartNew()
 # Windows PowerShell's native argument binder loses embedded quotes in a nested
 # -Command string. Encode only this test transport; do not change machine policy.
 $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
 $output=& powershell.exe -NoProfile -NonInteractive -EncodedCommand $encoded
 if($LASTEXITCODE){throw 'Inventory failed'}
 $apps=$output|ConvertFrom-Json
 $manual=@($apps|Where-Object {$_.path.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase)})
 Check ($manual.Count -eq 85 -and @($manual|Where-Object iconDataUri).Count -eq 85) 'All 85 manual folders receive icons beyond the old global limit'
 Check ($watch.Elapsed.TotalSeconds -lt 40) 'Full real inventory plus fixtures completes within the discovery deadline'
 $proof=[ordered]@{passed=$true;checks=@($checks);totalApps=$apps.Count;withIcons=@($apps|Where-Object iconDataUri).Count;manualIcons=$manual.Count;discoverySeconds=$watch.Elapsed.TotalSeconds;fixtures=$root}
 $proof|ConvertTo-Json -Depth 5|Set-Content -Encoding UTF8 (Join-Path $workspace 'artifacts\library-icons\test-proof.json')
 $proof|ConvertTo-Json -Depth 5
}finally{$env:SUPERHDD_LIBRARY_PROFILES=$oldProfiles}
