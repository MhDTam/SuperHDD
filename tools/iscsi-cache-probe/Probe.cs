// Private disposable-file integration probe; not an installer or physical disk backend.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using DiskAccessLibrary;
using ISCSI.Server;
using Microsoft.Win32.SafeHandles;
using SCSI;

internal sealed class CacheDisk : Disk
{
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_open(byte[] run, uint length);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_reopen(byte[] run, uint length);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_open_device(byte[] run,uint length,uint number);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_reopen_device(byte[] run,uint length,uint number);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    private static extern int cache_probe_read(ulong offset, [Out] byte[] buffer, uint length);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    private static extern int cache_probe_write(ulong offset, byte[] buffer, uint length);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_control(uint operation);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_watch(ulong offset,ulong length);
    [DllImport("superhdd_iscsi_probe.dll", CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_probe_close();
    private readonly int sectorBytes;
    internal CacheDisk() : this(4096) {}
    internal CacheDisk(int sectorBytes) {if(sectorBytes!=512 && sectorBytes!=4096)throw new ArgumentOutOfRangeException();this.sectorBytes=sectorBytes;}
    public override int BytesPerSector { get { return sectorBytes; } }
    public override long Size { get { return 128L * 1024 * 1024; } }
    public override bool IsReadOnly { get { return false; } }
    private void Bounds(long sector, int count)
    {
        if (sector < 0 || count <= 0 || count > 1048576/BytesPerSector || sector > Size / BytesPerSector - count)
            throw new ArgumentOutOfRangeException();
    }
    public override byte[] ReadSectors(long sector, int count)
    {
        Bounds(sector,count);
        byte[] data = new byte[count * BytesPerSector];
        if (cache_probe_read((ulong)(sector * BytesPerSector), data, (uint)data.Length) != 0)
            throw new IOException("SSD cache read failed");
        return data;
    }
    public override void WriteSectors(long sector, byte[] data)
    {
        if (data.Length % BytesPerSector != 0) throw new ArgumentOutOfRangeException();
        Bounds(sector,data.Length / BytesPerSector);
        if (cache_probe_write((ulong)(sector * BytesPerSector), data, (uint)data.Length) != 0)
            throw new IOException("Durable source write failed");
    }
}

internal static class NativeFile
{
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern SafeFileHandle CreateFile(string path,uint access,uint share,IntPtr security,uint creation,uint flags,IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern IntPtr VirtualAlloc(IntPtr address,UIntPtr size,uint allocation,uint protection);
    [DllImport("kernel32.dll")]
    private static extern bool VirtualFree(IntPtr address,UIntPtr size,uint type);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool SetFilePointerEx(SafeFileHandle file,long offset,out long position,uint method);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool ReadFile(SafeFileHandle file,IntPtr buffer,uint count,out uint done,IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool WriteFile(SafeFileHandle file,IntPtr buffer,uint count,out uint done,IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool FlushFileBuffers(SafeFileHandle file);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool DeviceIoControl(SafeFileHandle file,uint code,byte[] input,uint inputLength,byte[] output,uint outputLength,out uint returned,IntPtr overlapped);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern bool GetDiskFreeSpace(string root,out uint sectorsPerCluster,out uint bytesPerSector,out uint freeClusters,out uint clusters);
    private static void Check(bool ok) { if (!ok) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
    private static byte[] Pattern(int block)
    {
        byte[] data=new byte[65536];
        for(int i=0;i<data.Length;i++) data[i]=(byte)(block*37+i*17+i/251);
        return data;
    }
    internal static void Run(string mode,string path,string report)
    {
        // Client accepts only the private test mount and fixed fixture name.
        if(path != "S:\\fixture.bin") throw new ArgumentException("Private S test fixture required");
        bool seed=mode=="seed", mutate=mode=="mutate";
        using(SafeFileHandle file=CreateFile(path, seed||mutate ? 0xC0000000u : 0x80000000u,1,IntPtr.Zero,seed?1u:3u,0xA0000000u,IntPtr.Zero))
        {
            Check(!file.IsInvalid);
            IntPtr buffer=VirtualAlloc(IntPtr.Zero,new UIntPtr(65536),0x3000,4);
            Check(buffer!=IntPtr.Zero);
            try
            {
                long position;uint done;
                if(seed)
                {
                    for(int block=0;block<256;block++)
                    {
                        byte[] data=Pattern(block);Marshal.Copy(data,0,buffer,data.Length);
                        Check(WriteFile(file,buffer,(uint)data.Length,out done,IntPtr.Zero));Check(done==data.Length);
                    }
                    Check(FlushFileBuffers(file));
                    File.WriteAllText(report,"{\"passed\":true,\"bytes\":16777216,\"unbuffered\":true}");
                }
                else if(mode=="extents")
                {
                    uint sectors,sectorBytes,free,clusters,returned;
                    Check(GetDiskFreeSpace("S:\\",out sectors,out sectorBytes,out free,out clusters));
                    long clusterBytes=checked((long)sectors*sectorBytes);
                    byte[] map=new byte[65536];
                    Check(DeviceIoControl(file,0x90073,new byte[8],8,map,(uint)map.Length,out returned,IntPtr.Zero));
                    uint count=BitConverter.ToUInt32(map,0);
                    if(returned<16 || count==0 || count>128 || returned<16+count*16 || BitConverter.ToInt64(map,8)!=0)throw new IOException("Invalid fixture extent map");
                    long previous=0;StringBuilder json=new StringBuilder("{\"passed\":true,\"extents\":[");
                    for(int i=0;i<count;i++){
                        long next=BitConverter.ToInt64(map,16+i*16),lcn=BitConverter.ToInt64(map,24+i*16);
                        if(next<=previous || lcn<0)throw new IOException("Sparse or invalid fixture extent");
                        if(i!=0)json.Append(',');
                        json.Append("{\"volumeOffset\":").Append(checked(lcn*clusterBytes)).Append(",\"length\":").Append(checked((next-previous)*clusterBytes)).Append('}');
                        previous=next;
                    }
                    if(checked(previous*clusterBytes)!=16777216)throw new IOException("Fixture allocation length changed");
                    json.Append("]}");File.WriteAllText(report,json.ToString());
                }
                else if(mutate)
                {
                    Check(SetFilePointerEx(file,19L*65536,out position,0));
                    byte[] patch=new byte[4096];for(int i=0;i<patch.Length;i++)patch[i]=211;
                    Marshal.Copy(patch,0,buffer,patch.Length);
                    Check(WriteFile(file,buffer,4096,out done,IntPtr.Zero));Check(done==4096);Check(FlushFileBuffers(file));
                    Check(SetFilePointerEx(file,19L*65536,out position,0));
                    Check(ReadFile(file,buffer,65536,out done,IntPtr.Zero));Check(done==65536);
                    byte[] actual=new byte[65536];Marshal.Copy(buffer,actual,0,actual.Length);
                    byte[] expected=Pattern(19);Buffer.BlockCopy(patch,0,expected,0,patch.Length);
                    for(int i=0;i<actual.Length;i++)if(actual[i]!=expected[i])throw new IOException("Write invalidation mismatch");
                    File.WriteAllText(report,"{\"passed\":true,\"modifiedBytes\":4096,\"unbuffered\":true}");
                }
                else if(mode=="read" || mode=="read-mutated" || mode=="read-all-mutated")
                {
                    int requests=mode=="read-all-mutated"?256:96;
                    Stopwatch timer=Stopwatch.StartNew();
                    for(int request=0;request<requests;request++)
                    {
                        int block=(request*73+19)%256;
                        Check(SetFilePointerEx(file,(long)block*65536,out position,0));
                        Check(ReadFile(file,buffer,65536,out done,IntPtr.Zero));Check(done==65536);
                        byte[] actual=new byte[65536];Marshal.Copy(buffer,actual,0,actual.Length);
                        byte[] expected=Pattern(block);
                        if(mode!="read" && block==19)for(int i=0;i<4096;i++)expected[i]=211;
                        for(int i=0;i<actual.Length;i++)if(actual[i]!=expected[i])throw new IOException("Read oracle mismatch");
                    }
                    timer.Stop();
                    File.WriteAllText(report,"{\"passed\":true,\"requests\":"+requests+",\"bytes\":"+(requests*65536)+",\"unbuffered\":true,\"elapsedMs\":"+timer.ElapsedMilliseconds+"}");
                }
                else throw new ArgumentException("Unknown client mode");
            }
            finally { VirtualFree(buffer,UIntPtr.Zero,0x8000); }
        }
    }
}

internal static class Program
{
    private static int Main(string[] args)
    {
        try
        {
            if(args.Length==4 && args[0]=="client") { NativeFile.Run(args[1],args[2],args[3]);return 0; }
            if(args.Length==2 && args[0]=="protocol") { ProtocolProbe.Run(args[1]);return 0; }
            if(args.Length==3 && args[0]=="protocol-client") { ProtocolProbe.ClientPhase(args[1],args[2]);return 0; }
            if(args.Length==2 && args[0]=="protocol-denied") { ProtocolProbe.Denied(args[1]);return 0; }
            bool deviceMode=args.Length==3 && (args[0]=="server-device-512" || args[0]=="server-device-reopen-512");
            if((!deviceMode && (args.Length!=2 || !System.Text.RegularExpressions.Regex.IsMatch(args[0],"^server(-reopen)?(-512)?$"))) || !System.Text.RegularExpressions.Regex.IsMatch(args[1],"^[a-f0-9]{32}$"))
                throw new ArgumentException("server or server-reopen <owned run id> required");
            string run=args[1];
            string root=Path.GetFullPath(Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"..","..","artifacts","runtime","iscsi-local-"+run));
            Directory.CreateDirectory(root);
            int opened=deviceMode ? (args[0].Contains("reopen") ? CacheDisk.cache_probe_reopen_device(Encoding.ASCII.GetBytes(run),32,uint.Parse(args[2])) : CacheDisk.cache_probe_open_device(Encoding.ASCII.GetBytes(run),32,uint.Parse(args[2]))) : (args[0].Contains("reopen") ? CacheDisk.cache_probe_reopen(Encoding.ASCII.GetBytes(run),32) : CacheDisk.cache_probe_open(Encoding.ASCII.GetBytes(run),32));
            if(opened!=0)throw new IOException("Cannot open recorded disposable source");
            DiskAccessLibrary.Settings.MaximumTransferSizeLBA=256;
            ISCSIServer server=new ISCSIServer();
            string name="iqn.2026-10.local.superhdd:probe-"+run;
            ISCSITarget target=new ISCSITarget(name,new List<Disk>{new CacheDisk(args[0].EndsWith("-512")?512:4096)});
            if(File.Exists(Path.Combine(root,"kernel-initiator-only")))
            {
                server.AcceptConnection=delegate(IPEndPoint peer) {return KernelInitiatorPeer.Accept(peer,33260,Path.Combine(root,"peer-authorization.log"));};
                target.OnAuthorizationRequest += delegate(object sender,AuthorizationRequestArgs e) {e.Accept=KernelInitiatorPeer.Accept(e.InitiatorEndPoint,33260,Path.Combine(root,"peer-authorization.log"));};
            }
            target.OnUnitSerialNumberInquiry += delegate(object sender,UnitSerialNumberInquiryEventArgs e) {e.Page.ProductSerialNumber=run;};
            target.OnDeviceIdentificationInquiry += delegate(object sender,DeviceIdentificationInquiryEventArgs e) {
                e.Page.IdentificationDescriptorList.Clear();
                e.Page.IdentificationDescriptorList.Add(IdentificationDescriptor.GetSCSINameStringIdentifier(name));
            };
            server.AddTarget(target);
            server.Start(new IPEndPoint(IPAddress.Loopback,33260));
            File.WriteAllText(Path.Combine(root,"ready"),name);
            Stopwatch life=Stopwatch.StartNew();
            try
            {
                while(life.Elapsed < TimeSpan.FromMinutes(15))
                {
                    string commandPath=Path.Combine(root,"command");
                    if(File.Exists(commandPath))
                    {
                        string[] command=File.ReadAllText(commandPath).Trim().Split(' ');File.Delete(commandPath);
                        uint operation=uint.Parse(command[1]);
                        if(operation==5)
                        {
                            if(!server.RemoveTarget(name))throw new IOException("Initiator still connected; close refused");
                            File.WriteAllText(Path.Combine(root,"ack"),command[0]+" 0");break;
                        }
                        int result;
                        if(operation==6){
                            result=CacheDisk.cache_probe_control(4);
                            foreach(string line in File.ReadAllLines(Path.Combine(root,"watch-ranges"))){
                                string[] values=line.Trim().Split(' ');
                                ulong offset,length;
                                if(values.Length!=2 || !ulong.TryParse(values[0],out offset) || !ulong.TryParse(values[1],out length)){result=-1;break;}
                                result=CacheDisk.cache_probe_watch(offset,length);
                                if(result!=0)break;
                            }
                            if(result==0)result=CacheDisk.cache_probe_control(0);
                        }else result=CacheDisk.cache_probe_control(operation);
                        File.WriteAllText(Path.Combine(root,"ack"),command[0]+" "+result);
                    }
                    Thread.Sleep(50);
                }
            }
            finally { server.Stop();CacheDisk.cache_probe_control(0);CacheDisk.cache_probe_close(); }
            return 0;
        }
        catch(Exception error) {
            if(args.Length==4 && args[0]=="client") {
                System.ComponentModel.Win32Exception native=error as System.ComponentModel.Win32Exception;
                File.WriteAllText(args[3],"{\"passed\":false,\"win32Error\":"+(native==null?0:native.NativeErrorCode)+"}");
            }
            Console.Error.WriteLine(error);return 1;
        }
    }
}
