# Only the expressly approved .git metadata ACL repair. No storage operations.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Start-Transcript -LiteralPath (Join-Path $root 'artifacts\computer-use-repair.log') -Force | Out-Null
try{
 & (Join-Path $PSScriptRoot 'repair-computer-use-acl.ps1') -Apply
}catch{
 $_|Format-List * -Force|Out-String|Write-Output
 exit 1
}finally{Stop-Transcript|Out-Null}
