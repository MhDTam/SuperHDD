$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $root 'artifacts\cks-study'
$source = Join-Path $out 'upstream\EnableCustomKernelSigners'
$provenance = Get-Content -LiteralPath (Join-Path $out 'provenance.json') -Raw | ConvertFrom-Json
foreach ($file in $provenance.files) {
    if ((Get-FileHash -LiteralPath $file.Path).Hash -ne $file.Hash) { throw 'Upstream source hash mismatch' }
}
$vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio C++ tools are required' }
$vc = (Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
$sdk = 'C:\Program Files (x86)\Windows Kits\10'
$version = '10.0.26100.0'
$exe = Join-Path $out 'readonly-study.exe'
$args = @('/nologo', '/std:c++17', '/EHsc', '/W4', '/MT', '/DUNICODE', '/D_UNICODE', ('/I'+$source), ('/I'+(Join-Path $vc 'include')))
foreach ($part in @('ucrt', 'shared', 'um')) { $args += '/I'+(Join-Path $sdk "Include\$version\$part") }
# Explicit allowlist: the upstream setup/reboot entry point and kernel files are never built.
$args += @((Join-Path $PSScriptRoot 'readonly-study.cpp'), (Join-Path $source 'ProductPolicy.cpp'), (Join-Path $source 'ProductPolicyParser.cpp'), ('/Fo'+$out+'\'), ('/Fe'+$exe), '/link', ('/LIBPATH:'+(Join-Path $vc 'lib\x64')))
foreach ($part in @('ucrt','um')) { $args += '/LIBPATH:'+(Join-Path $sdk "Lib\$version\$part\x64") }
$args += 'advapi32.lib'
& (Join-Path $vc 'bin\Hostx64\x64\cl.exe') @args 2>&1 | Tee-Object -FilePath (Join-Path $out 'build.log')
if ($LASTEXITCODE) { throw 'Read-only harness compilation failed' }
$dumpbin = Join-Path $vc 'bin\Hostx64\x64\dumpbin.exe'
$imports = & $dumpbin /imports $exe
$imports | Set-Content -LiteralPath (Join-Path $out 'imports.txt') -Encoding utf8
if ($imports -match 'RegSetValue|RegCreateKey|RegDelete|InitiateSystemShutdown|ExitWindowsEx|AdjustTokenPrivileges|NtLoadDriver|ExUpdateLicenseData') {
    throw 'Unexpected mutating API imported; refusing execution'
}
$setupBefore = Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\Setup' | Select-Object CmdLine,SetupType
$json = & $exe
if ($LASTEXITCODE) { throw 'Read-only harness failed' }
$result = $json | ConvertFrom-Json
$setupAfter = Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\Setup' | Select-Object CmdLine,SetupType
$result | Add-Member -NotePropertyName setupValuesUnchangedDuringStudy -NotePropertyValue (($setupBefore | ConvertTo-Json -Compress) -eq ($setupAfter | ConvertTo-Json -Compress))
$result | Add-Member -NotePropertyName upstreamCommit -NotePropertyValue $provenance.sha
$result | Add-Member -NotePropertyName executableSha256 -NotePropertyValue (Get-FileHash -LiteralPath $exe).Hash
$result | Add-Member -NotePropertyName at -NotePropertyValue (Get-Date).ToUniversalTime().ToString('o')
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $out 'result.json') -Encoding utf8
$result | ConvertTo-Json -Depth 8
