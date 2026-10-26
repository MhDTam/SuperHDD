// Restrict the native test target to the Windows initiator's loopback connections.
// An initiator-supplied IQN/ISID is never an authentication credential.
using System;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
internal static class KernelInitiatorPeer
{
    [DllImport("iphlpapi.dll")]
    private static extern uint GetExtendedTcpTable(IntPtr table,ref uint bytes,bool order,uint family,uint tableClass,uint reserved);
    [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    private static extern IntPtr OpenSCManager(string machine,string database,uint access);
    [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
    private static extern IntPtr OpenService(IntPtr manager,string name,uint access);
    [DllImport("advapi32.dll",SetLastError=true)]
    private static extern bool QueryServiceStatusEx(IntPtr service,int level,byte[] buffer,uint size,out uint required);
    [DllImport("advapi32.dll")]
    private static extern bool CloseServiceHandle(IntPtr handle);
    [DllImport("kernel32.dll",SetLastError=true)]
    private static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
    [DllImport("kernel32.dll")]
    private static extern uint WaitForSingleObject(IntPtr handle,uint timeout);
    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);
    private static uint ServicePid()
    {
        IntPtr manager=OpenSCManager(null,null,1);if(manager==IntPtr.Zero)return 0;
        try
        {
            IntPtr service=OpenService(manager,"MSiSCSI",4);if(service==IntPtr.Zero)return 0;
            try
            {
                byte[] status=new byte[36];uint required;
                if(!QueryServiceStatusEx(service,0,status,36,out required) || required>36 || BitConverter.ToUInt32(status,4)!=4)return 0;
                return BitConverter.ToUInt32(status,28);
            }
            finally {CloseServiceHandle(service);}
        }
        finally {CloseServiceHandle(manager);}
    }
    private static bool IsWindowsInitiator(uint? owner)
    {
        if(!owner.HasValue || owner.Value==0)return false;
        if(owner.Value==4)return true;
        if(ServicePid()!=owner.Value)return false;
        // Pin the SCM-reported process while validating it again. PID reuse or
        // a stopped/replaced service must not authorize an unrelated process.
        IntPtr process=OpenProcess(0x00100000,false,owner.Value);if(process==IntPtr.Zero)return false;
        try {return WaitForSingleObject(process,0)==258 && ServicePid()==owner.Value;}
        finally {CloseHandle(process);}
    }
    private static int Port(uint network) {return (ushort)IPAddress.NetworkToHostOrder((short)(network&65535));}
    internal static uint? FindOwner(byte[] table,int used,IPEndPoint peer,int serverPort)
    {
        if(peer==null || !peer.Address.Equals(IPAddress.Loopback) || used<4 || used>table.Length)return null;
        uint count=BitConverter.ToUInt32(table,0);
        if(count>16384 || 4L+count*24L>used)return null;
        uint? owner=null;
        for(int i=0;i<count;i++)
        {
            int p=4+i*24;
            if(BitConverter.ToUInt32(table,p)!=5 || BitConverter.ToUInt32(table,p+4)!=0x0100007f ||
                Port(BitConverter.ToUInt32(table,p+8))!=peer.Port || BitConverter.ToUInt32(table,p+12)!=0x0100007f ||
                Port(BitConverter.ToUInt32(table,p+16))!=serverPort)continue;
            if(owner.HasValue)return null; // Ambiguous ownership never authorizes.
            owner=BitConverter.ToUInt32(table,p+20);
        }
        return owner;
    }
    internal static bool Accept(IPEndPoint peer,int serverPort,string log)
    {
        uint? owner=null;
        try
        {
            uint length=0;
            if(GetExtendedTcpTable(IntPtr.Zero,ref length,false,2,5,0)!=122)return false;
            for(int attempt=0;attempt<3;attempt++)
            {
                if(length<4 || length>1024*1024)return false;
                uint capacity=length;IntPtr memory=Marshal.AllocHGlobal((int)capacity);
                try
                {
                    uint result=GetExtendedTcpTable(memory,ref length,false,2,5,0);
                    if(result==122)continue;
                    if(result!=0 || length>capacity)return false;
                    byte[] table=new byte[length];Marshal.Copy(memory,table,0,(int)length);
                    owner=FindOwner(table,(int)length,peer,serverPort);break;
                }
                finally {Marshal.FreeHGlobal(memory);}
            }
        }
        catch {return false;}
        bool accepted=IsWindowsInitiator(owner);
        File.AppendAllText(log,"port="+peer.Port+" owner="+(owner.HasValue?owner.Value.ToString():"unknown")+" servicePid="+ServicePid()+" accepted="+accepted+Environment.NewLine);
        return accepted;
    }
    internal static void Test()
    {
        byte[] table=new byte[28];
        Action<int,uint> put=delegate(int offset,uint value){Buffer.BlockCopy(BitConverter.GetBytes(value),0,table,offset,4);};
        put(0,1);put(4,5);put(8,0x0100007f);put(12,(ushort)IPAddress.HostToNetworkOrder((short)12345));
        put(16,0x0100007f);put(20,(ushort)IPAddress.HostToNetworkOrder(unchecked((short)33260)));put(24,4);
        IPEndPoint peer=new IPEndPoint(IPAddress.Loopback,12345);
        if(FindOwner(table,28,peer,33260)!=4 || FindOwner(table,27,peer,33260).HasValue || FindOwner(table,28,new IPEndPoint(IPAddress.Loopback,12346),33260).HasValue)throw new IOException("Kernel peer tuple bounds failed");
        put(24,123);if(FindOwner(table,28,peer,33260)!=123)throw new IOException("Kernel peer PID parser failed");
        put(0,uint.MaxValue);if(FindOwner(table,28,peer,33260).HasValue)throw new IOException("Kernel peer count bounds failed");
    }
}
