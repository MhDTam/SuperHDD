param([switch]$VerifyReservation,[switch]$Scm)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$launched=[DateTime]::UtcNow
$launchState=Join-Path $root 'artifacts\loopback-runtime-launch-state.json'
[ordered]@{pid=$PID;started=$launched.ToString('o');phase='preflight';scm=[bool]$Scm}|ConvertTo-Json|Set-Content -LiteralPath $launchState -Encoding utf8NoBOM
Start-Transcript -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-run.log') -Force|Out-Null
try{
    if($Scm){
        & (Join-Path $PSScriptRoot 'install-loopback-lab-package.ps1') -UpdateOwned
    }
    & (Join-Path $PSScriptRoot 'test-loopback-runtime-lab.ps1') -VerifyReservation:$VerifyReservation -Scm:$Scm
    [ordered]@{pid=$PID;started=$launched.ToString('o');phase='finished';scm=[bool]$Scm;passed=$true}|ConvertTo-Json|Set-Content -LiteralPath $launchState -Encoding utf8NoBOM
}
catch{$_|Format-List * -Force|Out-String|Write-Output;[ordered]@{error=$_.Exception.Message;at=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-launch-error.json') -Encoding utf8NoBOM;[ordered]@{pid=$PID;started=$launched.ToString('o');phase='failed';scm=[bool]$Scm;passed=$false}|ConvertTo-Json|Set-Content -LiteralPath $launchState -Encoding utf8NoBOM;exit 1}
finally{Stop-Transcript|Out-Null}
