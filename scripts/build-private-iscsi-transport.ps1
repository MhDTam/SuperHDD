# Private source adapter. Original upstream/NuGet files stay intact.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$commit='8e31281a9c6d9f160c0ad996fd60358a0749b0e8'
$upstream=Join-Path $root ('.tools\iscsi-research\iSCSIConsole-'+$commit)
$source=Join-Path $upstream 'ISCSI\ISCSI.Server\ISCSIServer.cs'
if((Get-FileHash -LiteralPath $source).Hash -ne '81BBB691F7FEC5AC1C81E55B19DDBB8D40B967ECD769068BC61C019D3D99CED1'){throw 'Pinned upstream server source differs; refusing a blind patch.'}
$build=Join-Path $root '.tools\iscsi-private-build'
$out=Join-Path $root 'artifacts\iscsi-probe'
$null=New-Item -ItemType Directory -Path $build -Force
$original=[IO.File]::ReadAllText($source)
$needle='clientSocket.Send(response.GetBytes());'
if([regex]::Matches($original,[regex]::Escape($needle)).Count -ne 1){throw 'Expected one upstream send boundary.'}
$patched=Join-Path $build 'ISCSIServer.cs'
$adapted=$original.Replace($needle,'SuperHDDTransport.CompleteSocketSend.Send(clientSocket,response.GetBytes());')
$eventNeedle='public event EventHandler<LogEntry> OnLogEntry;'
$acceptNeedle='Log(Severity.Information, "New connection has been accepted");'
foreach($anchor in @($eventNeedle,$acceptNeedle)){if([regex]::Matches($original,[regex]::Escape($anchor)).Count -ne 1){throw 'Expected exactly one connection admission boundary.'}}
$adapted=$adapted.Replace($eventNeedle,$eventNeedle+[Environment]::NewLine+'        public Func<IPEndPoint,bool> AcceptConnection;'+[Environment]::NewLine+'        public Action<Socket,IPEndPoint> BindListener;')
$bindNeedle='m_listenerSocket.Bind(listenerEndPoint);'
if([regex]::Matches($original,[regex]::Escape($bindNeedle)).Count -ne 1){throw 'Expected one listener bind boundary.'}
$adapted=$adapted.Replace($bindNeedle,'if(BindListener==null)m_listenerSocket.Bind(listenerEndPoint);else BindListener(m_listenerSocket,listenerEndPoint);')
$admission=@'
bool admitted=false;
            try {admitted=AcceptConnection==null || AcceptConnection(clientSocket.RemoteEndPoint as IPEndPoint);}catch {admitted=false;}
            if(!admitted)
            {
                clientSocket.Close();
                try {m_listenerSocket.BeginAccept(ConnectRequestCallback,m_listenerSocket);}catch(ObjectDisposedException){}catch(SocketException){}
                return;
            }
            Log(Severity.Information, "New connection has been accepted");
'@
$adapted=$adapted.Replace($acceptNeedle,$admission)
[IO.File]::WriteAllText($patched,$adapted)
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$compiler=@(& $vswhere -latest -products '*' -find 'MSBuild\**\Roslyn\csc.exe')[0]
if(-not $compiler -or -not(Test-Path -LiteralPath $compiler)){throw 'Roslyn compiler required for pinned upstream source.'}
$sources=@(rg --files (Join-Path $upstream 'ISCSI') (Join-Path $upstream 'Utilities') -g '*.cs' | Where-Object {$_ -ne $source -and $_ -notmatch '\\(obj|bin)\\'})
$adapter=Join-Path $root 'tools\iscsi-cache-probe\CompleteSocketSend.cs'
$sources+=@($patched,$adapter)
$assembly=Join-Path $build 'AssemblyInfo.cs'
[IO.File]::WriteAllText($assembly,'[assembly:System.Reflection.AssemblyVersion("1.5.6.0")][assembly:System.Reflection.AssemblyTitle("ISCSI private Super HDD transport adapter")]')
$sources+=$assembly
$response=Join-Path $build 'compile.rsp'
$flags=@('/nologo','/target:library','/optimize+','/warnaserror+',('/out:"'+(Join-Path $out 'ISCSI.dll')+'"'),('/reference:"'+(Join-Path $out 'DiskAccessLibrary.dll')+'"'))
$flags+=@($sources|ForEach-Object{'"'+$_+'"'})
[IO.File]::WriteAllLines($response,$flags)
& $compiler ('@'+$response)
if($LASTEXITCODE){throw 'Private transport compilation failed.'}
[ordered]@{upstreamCommit=$commit;upstreamServerSha256=(Get-FileHash -LiteralPath $source).Hash;patch='CompleteSocketSend with bounded timeout and zero-progress rejection';sources=@($sources|ForEach-Object{Get-FileHash -LiteralPath $_|Select-Object Path,Hash});binary=Get-FileHash -LiteralPath (Join-Path $out 'ISCSI.dll')|Select-Object Path,Hash}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $out 'transport-build.json') -Encoding utf8NoBOM
