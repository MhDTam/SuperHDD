// Local protocol integration: no Windows initiator service or disk registration.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Text;
using DiskAccessLibrary;
using ISCSI.Client;
using ISCSI.Server;

internal static class ProtocolProbe
{
    internal static void Denied(string run)
    {
        if(!System.Text.RegularExpressions.Regex.IsMatch(run,"^[a-f0-9]{32}$"))throw new ArgumentException("Owned run required");
        ISCSIClient client=new ISCSIClient("iqn.2026-10.local.superhdd:untrusted-"+run);
        try
        {
            Require(client.Connect(IPAddress.Loopback,33260),"Cannot reach protected private target");
            bool loggedIn=false;
            try {loggedIn=client.Login("iqn.2026-10.local.superhdd:probe-"+run);}
            catch(System.Net.Sockets.SocketException){}
            catch(IOException){}
            Require(!loggedIn,"User-process connection bypassed Windows initiator restriction");
        }
        finally {client.Disconnect();}
    }
    private static void Require(bool value,string message)
    {
        if(!value)throw new IOException(message);
    }
    private static byte[] Pattern(int block)
    {
        byte[] data=new byte[65536];
        for(int i=0;i<data.Length;i++)data[i]=(byte)(block*37+i*17+i/251);
        return data;
    }
    private static void Equal(byte[] actual,byte[] expected)
    {
        Require(actual!=null && actual.Length==expected.Length,"Protocol read length mismatch");
        for(int i=0;i<actual.Length;i++)if(actual[i]!=expected[i])throw new IOException("Protocol byte oracle mismatch");
    }
    private static void Control(string root,uint operation,string tag)
    {
        Require(CacheDisk.cache_probe_control(operation)==0,"Cache control failed");
        File.Copy(Path.Combine(root,"stats.json"),Path.Combine(root,tag+".json"),true);
    }
    private static long Reads(ISCSIClient client,bool mutated=false,int requests=96)
    {
        Stopwatch timer=Stopwatch.StartNew();
        for(int i=0;i<requests;i++)
        {
            int block=(i*73+19)%256;
            byte[] expected=Pattern(block);
            if(mutated && block==19)for(int k=0;k<4096;k++)expected[k]=211;
            Equal(client.Read(0,(ulong)(block*16),16,4096),expected);
        }
        timer.Stop();return timer.ElapsedMilliseconds;
    }
    internal static void ClientPhase(string mode,string run)
    {
        if(!System.Text.RegularExpressions.Regex.IsMatch(run,"^[a-f0-9]{32}$") ||
            (mode!="seed" && mode!="mutate-hold" && mode!="read-all-mutated"))throw new ArgumentException("Owned protocol phase required");
        string root=Path.GetFullPath(Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"..","..","artifacts","runtime","iscsi-local-"+run));
        ISCSIClient client=new ISCSIClient("iqn.2026-10.local.superhdd:recovery-client-"+run);
        bool connected=false,loggedIn=false;
        try {
            connected=client.Connect(IPAddress.Loopback,33260);Require(connected,"Recovery client connection failed");
            loggedIn=client.Login("iqn.2026-10.local.superhdd:probe-"+run);Require(loggedIn,"Recovery client login failed");
            int sector;Require(client.ReadCapacity(0,out sector)==128UL*1024*1024 && sector==4096,"Unexpected recovery target capacity");
            if(mode=="seed"){
                for(int block=0;block<256;block++)Require(client.Write(0,(ulong)(block*16),Pattern(block),4096),"Recovery seed write failed");
                File.WriteAllText(Path.Combine(root,mode+".json"),"{\"passed\":true,\"bytes\":16777216,\"writes\":256}");
            }else if(mode=="mutate-hold"){
                byte[] patch=new byte[4096];for(int i=0;i<patch.Length;i++)patch[i]=211;
                Require(client.Write(0,19*16,patch,4096),"Recovery mutation write failed");
                byte[] expected=Pattern(19);Buffer.BlockCopy(patch,0,expected,0,patch.Length);
                Equal(client.Read(0,19*16,16,4096),expected);
                File.WriteAllText(Path.Combine(root,mode+".json"),"{\"passed\":true,\"modifiedBytes\":4096,\"sessionHeld\":true}");
                Stopwatch wait=Stopwatch.StartNew();
                while(!File.Exists(Path.Combine(root,"release-client")) && wait.Elapsed<TimeSpan.FromSeconds(60))System.Threading.Thread.Sleep(50);
                Require(File.Exists(Path.Combine(root,"release-client")),"Held client release timed out");
                // The owner deliberately killed the target while we stayed connected.
                client.Disconnect();connected=false;loggedIn=false;
            }else{
                long elapsed=Reads(client,true,256);
                File.WriteAllText(Path.Combine(root,mode+".json"),"{\"passed\":true,\"requests\":256,\"bytes\":16777216,\"elapsedMs\":"+elapsed+"}");
            }
            if(loggedIn){Require(client.Logout(),"Recovery client logout failed");loggedIn=false;}
            if(connected){client.Disconnect();connected=false;}
        }finally{
            if(loggedIn){try{client.Logout();}catch{}}
            if(connected){try{client.Disconnect();}catch{}}
        }
    }
    internal static void Run(string run)
    {
        if(!System.Text.RegularExpressions.Regex.IsMatch(run,"^[a-f0-9]{32}$"))throw new ArgumentException("Owned run id required");
        KernelInitiatorPeer.Test();
        byte[] frame=Pattern(13);
        foreach(int limit in new int[]{1,7,511,16384})
        {
            MemoryStream collected=new MemoryStream();int calls=0;
            SuperHDDTransport.CompleteSocketSend.SendAll(frame,delegate(byte[] bytes,int offset,int count) {int sent=Math.Min(limit,count);collected.Write(bytes,offset,sent);calls++;return sent;});
            Equal(collected.ToArray(),frame);Require(calls>1,"Short-send test did not force multiple sends");
        }
        foreach(int invalid in new int[]{0,-1,65537})
        {
            bool refused=false;
            try {SuperHDDTransport.CompleteSocketSend.SendAll(frame,delegate(byte[] bytes,int offset,int count){return invalid;});}
            catch(System.Net.Sockets.SocketException){refused=true;}
            Require(refused,"Invalid socket progress accepted");
        }
        string root=Path.GetFullPath(Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"..","..","artifacts","runtime","iscsi-local-"+run));
        Directory.CreateDirectory(root);
        string source="D:\\SuperHDD-iSCSI-"+run+".img";
        string cache=Path.Combine(root,"ssd-cache.bin");
        bool opened=false,connected=false,loggedIn=false;
        ISCSIServer server=new ISCSIServer();
        ISCSIClient client=new ISCSIClient("iqn.2026-10.local.superhdd:protocol-client-"+run);
        try
        {
            Require(CacheDisk.cache_probe_open(Encoding.ASCII.GetBytes(run),32)==0,"New disposable source open failed");
            opened=true;
            // Exercise the 512-byte RMW path across 4 KiB boundaries and assert
            // that untouched neighboring sectors retain their original bytes.
            CacheDisk smallSectors=new CacheDisk(512);
            byte[] original=new byte[8192];for(int i=0;i<original.Length;i++)original[i]=(byte)(i*23);
            smallSectors.WriteSectors(0,original);
            byte[] partial=new byte[1024];for(int i=0;i<partial.Length;i++)partial[i]=173;
            smallSectors.WriteSectors(7,partial);
            Buffer.BlockCopy(partial,0,original,7*512,partial.Length);
            Equal(smallSectors.ReadSectors(0,16),original);
            Control(root,1,"watch-cache-cleared");
            Control(root,4,"watch-reset");
            Require(CacheDisk.cache_probe_watch(0,8192)==0,"Fixture observer registration failed");
            Require(CacheDisk.cache_probe_watch(512,512)!=0,"Overlapping observer accepted");
            Require(CacheDisk.cache_probe_watch(128UL*1024*1024,512)!=0,"Out-of-bounds observer accepted");
            Equal(smallSectors.ReadSectors(0,16),original);Control(root,0,"watch-inside");
            Equal(smallSectors.ReadSectors(128,8),new byte[4096]);Control(root,0,"watch-outside");
            Control(root,4,"watch-finished");
            DiskAccessLibrary.Settings.MaximumTransferSizeLBA=256;
            string name="iqn.2026-10.local.superhdd:protocol-"+run;
            server.AddTarget(new ISCSITarget(name,new List<Disk>{new CacheDisk()}));
            server.Start(new IPEndPoint(IPAddress.Loopback,33261));
            connected=client.Connect(IPAddress.Loopback,33261);
            Require(connected,"Local protocol connection failed");
            loggedIn=client.Login(name);Require(loggedIn,"Local protocol login failed");
            Require(client.GetLUNList().Contains(0),"Expected LUN 0");
            int sector;
            Require(client.ReadCapacity(0,out sector)==128UL*1024*1024 && sector==4096,"4Kn capacity mismatch");
            bool excluded=false;
            try {using(FileStream file=new FileStream(source,FileMode.Open,FileAccess.ReadWrite,FileShare.ReadWrite)) {}}
            catch(IOException) {excluded=true;}
            Require(excluded,"External source handle was not excluded");
            for(int block=0;block<256;block++)Require(client.Write(0,(ulong)(block*16),Pattern(block),4096),"Durable protocol write failed");
            Control(root,1,"cleared");
            long cold=Reads(client);Control(root,0,"cold");
            long warm=Reads(client);Control(root,0,"warm");
            Control(root,2,"pause-start");Reads(client);Control(root,0,"paused");
            Control(root,3,"resumed");
            byte[] patch=new byte[4096];for(int i=0;i<patch.Length;i++)patch[i]=211;
            Require(client.Write(0,19*16,patch,4096),"Partial-sector source write failed");
            byte[] expected=Pattern(19);Buffer.BlockCopy(patch,0,expected,0,patch.Length);
            Equal(client.Read(0,19*16,16,4096),expected);
            Control(root,0,"mutated");
            bool readRejected=false;
            try {readRejected=client.Read(0,128UL*1024*1024/4096,1,4096)==null;}
            catch(InvalidDataException) {readRejected=true;} // This client throws on a SCSI sense response.
            Require(readRejected,"Out-of-range read was accepted");
            Require(!client.Write(0,128UL*1024*1024/4096,new byte[4096],4096),"Out-of-range write was accepted");
            Equal(client.Read(0,19*16,16,4096),expected);
            Control(root,0,"final");
            Require(client.Logout(),"Protocol logout failed");loggedIn=false;
            client.Disconnect();connected=false;
            Require(server.RemoveTarget(name),"Session remained after logout");
            File.WriteAllText(Path.Combine(root,"protocol.json"),"{\"passed\":true,\"requestsPerPass\":96,\"oracleBytesPerPass\":6291456,\"coldMs\":"+cold+",\"warmMs\":"+warm+",\"sourceHandleExcluded\":true,\"outOfRangeRejected\":true,\"scope\":\"User-mode loopback protocol only; Windows initiator and existing D volume are not intercepted\"}");
        }
        finally
        {
            if(loggedIn){try {client.Logout();}catch {}}
            if(connected){try {client.Disconnect();}catch {}}
            server.Stop();
            if(opened)
            {
                CacheDisk.cache_probe_control(0);
                Require(CacheDisk.cache_probe_close()==0,"Source release failed");
                File.Delete(source);File.Delete(cache);
            }
        }
    }
}
