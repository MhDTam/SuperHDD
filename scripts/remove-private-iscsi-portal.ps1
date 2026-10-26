# Windows 10's Remove-IscsiTargetPortal CDXML declares Int32, but its CIM
# provider requires UInt16. Use the actual local provider signature and only
# the probe's exact loopback address/port, preserving other portals.
function Remove-PrivateIscsiPortal {
    $portals=@(Get-CimInstance -Namespace root/Microsoft/Windows/Storage -ClassName MSFT_iSCSITargetPortal -Filter "TargetPortalAddress = '127.0.0.1' AND TargetPortalPortNumber = 33260")
    foreach($portal in $portals){
        $arguments=@{
            TargetPortalAddress=[string]$portal.TargetPortalAddress
            TargetPortalPortNumber=[uint16]$portal.TargetPortalPortNumber
        }
        if($portal.InitiatorInstanceName){$arguments.InitiatorInstanceName=[string]$portal.InitiatorInstanceName}
        if($portal.InitiatorPortalAddress){$arguments.InitiatorPortalAddress=[string]$portal.InitiatorPortalAddress}
        $reply=Invoke-CimMethod -InputObject $portal -MethodName Remove -Arguments $arguments
        if($reply.ReturnValue -ne 0){throw ('Private portal removal failed: '+$reply.ReturnValue)}
    }
}
