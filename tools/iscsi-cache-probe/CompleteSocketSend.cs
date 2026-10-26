// Super HDD adapter around the dynamically linked LGPL transport.
using System;
using System.Net.Sockets;
namespace SuperHDDTransport
{
    public static class CompleteSocketSend
    {
        public static void Send(Socket socket,byte[] payload)
        {
            socket.SendTimeout=15000;
            try { SendAll(payload,delegate(byte[] bytes,int offset,int count) {return socket.Send(bytes,offset,count,SocketFlags.None);}); }
            catch { socket.Close();throw; }
        }
        // Test the actual short-send boundary independently of socket buffers.
        public static void SendAll(byte[] payload,Func<byte[],int,int,int> send)
        {
            int offset=0;
            while(offset<payload.Length)
            {
                int count=send(payload,offset,payload.Length-offset);
                if(count<=0 || count>payload.Length-offset)throw new SocketException((int)SocketError.ConnectionReset);
                offset+=count;
            }
        }
    }
}
