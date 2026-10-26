$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'enter-dev.ps1')
$previousFlags=$env:RUSTFLAGS
try{
    # A pristine Server Core guest has no Visual C++ redistributable. Keep the
    # lab tools self-contained; this does not change desktop release settings.
    $env:RUSTFLAGS='-C target-feature=+crt-static'
    Push-Location $root
    try{
        cargo build --locked --offline -p superhdd-platform --example raw_lab --example pnp_lab --example pnp_probe --example raw_benchmark --release
        if($LASTEXITCODE){throw 'Portable lab-tool compilation failed.'}
    }finally{Pop-Location}
}finally{$env:RUSTFLAGS=$previousFlags}
