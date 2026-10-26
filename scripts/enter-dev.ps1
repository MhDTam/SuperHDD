$taskRoot=Split-Path -Parent $PSScriptRoot
if(Test-Path -LiteralPath (Join-Path $taskRoot '.tools\cargo\bin\cargo.exe')){
    $env:CARGO_HOME=Join-Path $taskRoot '.tools\cargo'
    $env:RUSTUP_HOME=Join-Path $taskRoot '.tools\rustup'
    $env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
}
$env:CARGO_BUILD_JOBS='4'
