param([switch]$Apply)
# Direct maintenance of .git's ACL only. No parent ACL or ownership changes.
$ErrorActionPreference='Stop'
$root=(Get-Item -LiteralPath (Split-Path -Parent $PSScriptRoot)).FullName.TrimEnd('\')
if($root -ne 'M:\Codex\Super HDD'){throw 'Unexpected workspace.'}
$git=Join-Path $root '.git'
$item=Get-Item -LiteralPath $git -Force
if(-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Regular .git directory required.'}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$sid=$identity.User
$parent=Get-Acl -LiteralPath $root
$before=Get-Acl -LiteralPath $git
if($parent.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value){throw 'Only the existing workspace owner may repair this ACL.'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
function Dacl($acl){$acl.GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)}
function OtherRules($acl){
 $entries=@($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]))
 [string[]]$rules=@(foreach($entry in $entries){
   if($entry.IdentityReference.Value -ne $sid.Value -or $entry.AccessControlType -eq 'Deny'){
     '{0}|{1}|{2}|{3}|{4}|{5}' -f $entry.IdentityReference.Value,[int]$entry.FileSystemRights,[int]$entry.AccessControlType,$entry.IsInherited,[int]$entry.InheritanceFlags,[int]$entry.PropagationFlags
   }
 })
 # Windows may reorder inherited allow ACEs while preserving their byte content.
 # Only allow-only ACLs can use this unordered comparison; deny ACLs retain order.
 if(-not @($entries|Where-Object AccessControlType -eq 'Deny').Count){[Array]::Sort($rules,[StringComparer]::Ordinal)}
 $rules -join '|'
}
if(-not $Apply){[pscustomobject]@{target=$git;elevated=$admin;owner=$before.Owner;repair='Add READ_CONTROL/WRITE_DAC for workspace owner on .git only, preserving all other rules'}|ConvertTo-Json;return}
if(-not $admin){throw 'Normal Windows administrator approval required; no ACL changes made.'}
$record=Join-Path $root ('artifacts\runtime\computer-use-direct-repair-'+[Guid]::NewGuid().ToString('N')+'.json')
$parentDacl=Dacl $parent
$otherRules=OtherRules $before
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');target=$git;userSid=$sid.Value;gitDaclBefore=(Dacl $before);ownerBefore=$before.GetOwner([Security.Principal.SecurityIdentifier]).Value;applied=$false;passed=$false}
$result|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $record -Encoding utf8NoBOM
try{
 $rights=[Security.AccessControl.FileSystemRights]::ReadPermissions -bor [Security.AccessControl.FileSystemRights]::ChangePermissions
 $rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,$rights,[Security.AccessControl.InheritanceFlags]::None,[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow)
 $updated=Get-Acl -LiteralPath $git
 $updated.AddAccessRule($rule)
 Set-Acl -LiteralPath $git -AclObject $updated
 $result.applied=$true
 $after=Get-Acl -LiteralPath $git
 $result.gitDaclAfter=Dacl $after
 $result.otherPrincipalsAndDeniesUnchanged=((OtherRules $after) -eq $otherRules)
 $result.ownerUnchanged=($after.GetOwner([Security.Principal.SecurityIdentifier]).Value -eq $result.ownerBefore)
 $result.inheritanceUnchanged=($before.AreAccessRulesProtected -eq $after.AreAccessRulesProtected)
 $result.parentUnchanged=((Dacl (Get-Acl -LiteralPath $root)) -eq $parentDacl)
 $result.passed=[bool]($result.otherPrincipalsAndDeniesUnchanged -and $result.ownerUnchanged -and $result.inheritanceUnchanged -and $result.parentUnchanged)
}catch{$result.error=$_.Exception.Message}finally{$result|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
[pscustomobject]@{passed=$result.passed;applied=$result.applied;error=$result.error;record=$record}|ConvertTo-Json
if(-not $result.passed){throw 'Direct ACL maintenance did not pass.'}
