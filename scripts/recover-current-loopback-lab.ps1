# Preserve evidence before the strictly owned disposable-source recovery.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$proof=Get-Content -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-lab.json') -Raw|ConvertFrom-Json
$run=$proof.runId
if($run -notmatch '^[a-f0-9]{32}$' -or $proof.passed){throw 'A failed disposable run is required.'}
Copy-Item -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-lab.json') -Destination (Join-Path $root ('artifacts\loopback-runtime-lab-'+$run+'.json'))
try{& (Join-Path $PSScriptRoot 'inspect-loopback-lab.ps1')}catch{
    $_.Exception.Message|Set-Content -LiteralPath (Join-Path $root 'artifacts\loopback-scm-diagnostic-error.log')
}
& (Join-Path $PSScriptRoot 'recover-loopback-runtime-lab.ps1') -Run $run
$recovery=Get-Content -LiteralPath (Join-Path $root ('artifacts\loopback-runtime-recovery-'+$run+'.json')) -Raw|ConvertFrom-Json
if(-not $recovery.passed){throw 'Owned disposable recovery must finish before continuing.'}
# Continue the already requested setup in this same elevated session.
& (Join-Path $PSScriptRoot 'complete-local-native-cache.ps1')
