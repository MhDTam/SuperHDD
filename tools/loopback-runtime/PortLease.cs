using System;
using System.Net;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Security.Principal;
internal static class PortLease
{
    internal const int Port=33262;
    private const int Associate=unchecked((int)0x98000066);
    [DllImport("iphlpapi.dll")]
    private static extern uint LookupPersistentTcpPortReservation(ushort start,ushort count,out ulong token);
    internal static void Bind(Socket socket,ulong token)
    {
        if(token==0)throw new InvalidOperationException("Missing protected endpoint reservation");
        socket.ExclusiveAddressUse=true;
        socket.IOControl(Associate,BitConverter.GetBytes(token),null);
        socket.Bind(new IPEndPoint(IPAddress.Loopback,0));
        if(((IPEndPoint)socket.LocalEndPoint).Port!=Port)throw new InvalidOperationException("Reserved listener selected a different endpoint");
    }
    internal static int Probe()
    {
        if(new WindowsPrincipal(WindowsIdentity.GetCurrent()).IsInRole(WindowsBuiltInRole.Administrator))return 2;
        bool bindDenied=false,associationDenied=false;
        using(Socket socket=new Socket(AddressFamily.InterNetwork,SocketType.Stream,ProtocolType.Tcp))
        {
            try{socket.Bind(new IPEndPoint(IPAddress.Loopback,Port));}
            catch(SocketException error){bindDenied=error.SocketErrorCode==SocketError.AccessDenied;}
        }
        ulong token;uint lookup=LookupPersistentTcpPortReservation(unchecked((ushort)IPAddress.HostToNetworkOrder((short)Port)),1,out token);
        if(lookup==5)associationDenied=true;
        else if(lookup==0)
        {
            using(Socket socket=new Socket(AddressFamily.InterNetwork,SocketType.Stream,ProtocolType.Tcp))
            {
                try{socket.IOControl(Associate,BitConverter.GetBytes(token),null);}
                catch(SocketException error){associationDenied=error.SocketErrorCode==SocketError.AccessDenied;}
            }
        }
        Console.WriteLine("{\"nonAdmin\":true,\"bindDenied\":"+bindDenied.ToString().ToLowerInvariant()+",\"associationDenied\":"+associationDenied.ToString().ToLowerInvariant()+",\"passed\":"+(bindDenied&&associationDenied).ToString().ToLowerInvariant()+"}");
        return bindDenied&&associationDenied?0:1;
    }
}
