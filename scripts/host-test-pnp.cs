// Per-device filter property only. Never edits disk/volume class-wide filters.
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class SuperHDDHostPnp {
 const string Filter="SuperHDDHostTest";
 [StructLayout(LayoutKind.Sequential)] struct Info {public uint Size;public Guid Class;public uint DevInst;public UIntPtr Reserved;}
 [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(IntPtr g,IntPtr w);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiOpenDeviceInfo(IntPtr h,string id,IntPtr w,uint flags,ref Info i);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiGetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,out uint type,byte[] b,uint n,out uint required);
 [DllImport("setupapi.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool SetupDiSetDeviceRegistryProperty(IntPtr h,ref Info i,uint p,byte[] b,uint n);
 [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr h);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint QueryDosDevice(string s,StringBuilder b,int n);
 public static string Device(string letter){var b=new StringBuilder(1024);if(QueryDosDevice(letter,b,b.Capacity)==0)throw new Win32Exception();return b.ToString();}
 public static void SetFilter(string instance,string expectedPdo,bool enroll){
  if(!instance.StartsWith("STORAGE\\Volume\\",StringComparison.OrdinalIgnoreCase))throw new Exception("Not a volume instance");
  var h=SetupDiCreateDeviceInfoList(IntPtr.Zero,IntPtr.Zero);if(h==new IntPtr(-1))throw new Win32Exception();
  try{
   var i=new Info{Size=(uint)Marshal.SizeOf(typeof(Info))};if(!SetupDiOpenDeviceInfo(h,instance,IntPtr.Zero,0,ref i))throw new Win32Exception();
   uint type,n;var b=new byte[4096];
   if(enroll){
    if(!SetupDiGetDeviceRegistryProperty(h,ref i,14,out type,b,(uint)b.Length,out n))throw new Win32Exception();
    if(!String.Equals(Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0'),expectedPdo,StringComparison.OrdinalIgnoreCase))throw new Exception("Source PDO changed");
   }
   Array.Clear(b,0,b.Length);
   string old="";
   if(SetupDiGetDeviceRegistryProperty(h,ref i,17,out type,b,(uint)b.Length,out n)){
    if(type!=7)throw new Exception("Unexpected filter property type");old=Encoding.Unicode.GetString(b,0,(int)n).TrimEnd('\0');
   }else if(Marshal.GetLastWin32Error()!=13)throw new Win32Exception();
   if(old.Length!=0&&!String.Equals(old,Filter,StringComparison.Ordinal))throw new Exception("Preserve another existing filter");
   if(enroll){var bytes=Encoding.Unicode.GetBytes(Filter+"\0\0");if(!SetupDiSetDeviceRegistryProperty(h,ref i,17,bytes,(uint)bytes.Length))throw new Win32Exception();}
   else if(old.Length!=0&&!SetupDiSetDeviceRegistryProperty(h,ref i,17,null,0))throw new Win32Exception();
  }finally{SetupDiDestroyDeviceInfoList(h);}
 }
}
