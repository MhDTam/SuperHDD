// Fixed protected-runtime worker. Source/device selection belongs to the native
// configuration and prepared service journal, never to command-line device paths.
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
using SCSI;
internal sealed class RuntimeDisk:Disk
{
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_runtime_open(byte[] run,uint length);
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    private static extern ulong cache_runtime_size();
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    private static extern int cache_runtime_read(ulong offset,[Out]byte[] buffer,uint length);
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    private static extern int cache_runtime_write(ulong offset,byte[] buffer,uint length);
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_runtime_control(uint operation);
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    internal static extern int cache_runtime_close();
    [DllImport("superhdd_loopback_runtime.dll",CallingConvention=CallingConvention.Cdecl)]
    internal static extern ulong cache_runtime_port_token();
    private readonly long size;
    internal RuntimeDisk(){ulong bytes=cache_runtime_size();if(bytes==0 || bytes>long.MaxValue || bytes%4096!=0)throw new IOException("Invalid owned source geometry");size=(long)bytes;}
    public override long Size {get{return size;}}
    public override int BytesPerSector {get{return 512;}}
    public override bool IsReadOnly {get{return false;}}
    private void Bounds(long sector,int count){if(sector<0 || count<=0 || count>1048576/512 || sector>size/512-count)throw new ArgumentOutOfRangeException();}
    public override byte[] ReadSectors(long sector,int count){Bounds(sector,count);byte[] data=new byte[count*512];if(cache_runtime_read((ulong)sector*512,data,(uint)data.Length)!=0)throw new IOException("Authoritative cache read failed");return data;}
    public override void WriteSectors(long sector,byte[] data){if(data==null || data.Length%512!=0)throw new ArgumentOutOfRangeException();Bounds(sector,data.Length/512);if(cache_runtime_write((ulong)sector*512,data,(uint)data.Length)!=0)throw new IOException("Durable source write failed");}
}
internal static class Program
{
    // The supervisor publishes this mailbox with a short-lived write/delete
    // handle. A poll can race that handle or its deletion; neither condition
    // means the data-serving worker should exit.
    private static string TakeCommand(string path)
    {
        try
        {
            string text=File.ReadAllText(path);
            File.Delete(path);
            return text;
        }
        catch(IOException error)
        {
            int code=error.HResult&0xffff;
            if(code==2 || code==32 || code==33)return null;
            throw;
        }
    }
    private static int Main(string[] args)
    {
        if(args.Length==1 && args[0]=="--probe-port")return PortLease.Probe();
        if(args.Length!=1 || !System.Text.RegularExpressions.Regex.IsMatch(args[0],"^[a-f0-9]{32}$"))return 2;
        string run=args[0];string root=Path.Combine(@"C:\ProgramData\SuperHDDLoopback",run);
        string name="iqn.2026-10.local.superhdd:cache-"+run;
        ISCSIServer server=new ISCSIServer();bool opened=false;
        try
        {
            if(RuntimeDisk.cache_runtime_open(Encoding.ASCII.GetBytes(run),32)!=0)throw new IOException("Protected prepared runtime could not open");
            opened=true;
            DiskAccessLibrary.Settings.MaximumTransferSizeLBA=256;
            RuntimeDisk disk=new RuntimeDisk();
            ISCSITarget target=new ISCSITarget(name,new List<Disk>{disk});
            target.OnUnitSerialNumberInquiry+=delegate(object sender,UnitSerialNumberInquiryEventArgs e){e.Page.ProductSerialNumber=run;};
            target.OnDeviceIdentificationInquiry+=delegate(object sender,DeviceIdentificationInquiryEventArgs e){e.Page.IdentificationDescriptorList.Clear();e.Page.IdentificationDescriptorList.Add(IdentificationDescriptor.GetSCSINameStringIdentifier(name));};
            // Admission runs before all PDUs, including discovery/reinstatement.
            server.AcceptConnection=delegate(IPEndPoint peer){return KernelInitiatorPeer.Accept(peer,PortLease.Port,Path.Combine(root,"peer-authorization.log"));};
            server.BindListener=delegate(System.Net.Sockets.Socket socket,IPEndPoint endpoint){PortLease.Bind(socket,RuntimeDisk.cache_runtime_port_token());};
            server.AddTarget(target);server.Start(new IPEndPoint(IPAddress.Loopback,PortLease.Port));
            if(RuntimeDisk.cache_runtime_control(0)!=0)throw new IOException("Cannot publish initial owned runtime state");
            Process process=Process.GetCurrentProcess();
            File.WriteAllText(Path.Combine(root,"ready.json"),"{\"schema\":1,\"runId\":\""+run+"\",\"pid\":"+process.Id+",\"created\":"+process.StartTime.ToUniversalTime().ToFileTimeUtc()+",\"sourceBytes\":"+disk.Size+"}");
            bool stopped=false;
            while(!stopped)
            {
                string path=Path.Combine(root,"command");
                string text=TakeCommand(path);
                if(text!=null)
                {
                    if(text.Length<=128)
                    {
                        string[] parts=text.Trim().Split(' ');uint operation;
                        if(parts.Length==2 && System.Text.RegularExpressions.Regex.IsMatch(parts[0],"^[a-f0-9]{32}$") && uint.TryParse(parts[1],out operation) && operation<=5)
                        {
                            int result=-1;
                            if(operation<=3)result=RuntimeDisk.cache_runtime_control(operation);
                            else if(operation==5 && server.RemoveTarget(name)){result=0;stopped=true;}
                            // A busy target refuses stop and remains alive.
                            File.WriteAllText(Path.Combine(root,"ack"),parts[0]+" "+result);
                        }
                    }
                }
                Thread.Sleep(25);
            }
            return 0;
        }
        catch(Exception error){Console.Error.WriteLine(error);return 1;}
        finally{server.Stop();if(opened)RuntimeDisk.cache_runtime_close();}
    }
}
