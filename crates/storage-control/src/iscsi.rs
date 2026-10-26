//! Native Microsoft initiator control for our exact loopback target namespace.
//! No shell commands, remote portals, persistent logins or global portal changes.
//! The caller must offline/dismount the owned projected disk before logout.
use serde::{Deserialize,Serialize};
use std::{mem::size_of,ptr::{null,null_mut}};
use windows_sys::Win32::{Foundation::{ERROR_INSUFFICIENT_BUFFER,ERROR_SUCCESS},Storage::IscsiDisc::*};
const PREFIX:&str="iqn.2026-10.local.superhdd:probe-";
const MANAGED_PREFIX:&str="iqn.2026-10.local.superhdd:cache-";
pub const PORT:u16=33260;
fn target_port(target:&str)->u16 {if target.starts_with(MANAGED_PREFIX){crate::port_reservation::PORT}else{PORT}}
fn error(operation:&str,code:u32)->String{format!("{operation}: 0x{code:08x} ({})",std::io::Error::from_raw_os_error(code as i32))}
pub fn target_name(run:&str)->Result<String,String>{
    if run.len()!=32 || !run.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)){return Err("One lower-case owned run ID required".into());}
    Ok(format!("{PREFIX}{run}"))
}
fn validate_target(target:&str)->Result<(),String>{
    if let Some(run)=target.strip_prefix(PREFIX){if target_name(run)?==target{return Ok(());}}
    if let Some(run)=target.strip_prefix(MANAGED_PREFIX){if managed_target_name(run)?==target{return Ok(());}}
    Err("Only an exact SuperHDD loopback target is accepted".into())
}
pub fn managed_target_name(run:&str)->Result<String,String>{target_name(run)?;Ok(format!("{MANAGED_PREFIX}{run}"))}
fn wide(value:&str)->Vec<u16>{value.encode_utf16().chain(Some(0)).collect()}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct SessionId{pub adapter_unique:u64,pub adapter_specific:u64}
impl SessionId{
    fn native(self)->ISCSI_UNIQUE_SESSION_ID{ISCSI_UNIQUE_SESSION_ID{AdapterUnique:self.adapter_unique,AdapterSpecific:self.adapter_specific}}
}
impl From<ISCSI_UNIQUE_SESSION_ID> for SessionId{fn from(id:ISCSI_UNIQUE_SESSION_ID)->Self{Self{adapter_unique:id.AdapterUnique,adapter_specific:id.AdapterSpecific}}}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Connection{pub target_address:String,pub target_port:u16}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Session{pub id:SessionId,pub target:String,pub connections:Vec<Connection>}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Device{pub disk_number:u32,pub device_type:u32,pub legacy_name:String,pub interface_name:String}
fn checked_offset(buffer:&[u64],used:usize,pointer:usize,length:usize,alignment:usize)->Result<usize,String>{
    let base=buffer.as_ptr() as usize;
    let offset=pointer.checked_sub(base).ok_or("Native pointer before buffer")?;
    if used>buffer.len()*8 || !pointer.is_multiple_of(alignment) || offset.checked_add(length).is_none_or(|end|end>used){return Err("Native pointer outside returned buffer".into());}
    Ok(offset)
}
fn string_in(buffer:&[u64],used:usize,pointer:*const u16)->Result<String,String>{
    let offset=checked_offset(buffer,used,pointer as usize,2,2)?;
    let count=((used-offset)/2).min(32768);
    let chars=unsafe{std::slice::from_raw_parts(pointer,count)};
    let end=chars.iter().position(|c|*c==0).ok_or("Unterminated native string")?;
    String::from_utf16(&chars[..end]).map_err(|e|e.to_string())
}
fn fixed_string(chars:&[u16])->Result<String,String>{
    let end=chars.iter().position(|c|*c==0).ok_or("Unterminated fixed native string")?;
    String::from_utf16(&chars[..end]).map_err(|e|e.to_string())
}
pub fn sessions()->Result<Vec<Session>,String>{
    let mut bytes=0;let mut count=0;
    let code=unsafe{GetIScsiSessionListW(&mut bytes,&mut count,null_mut())};
    if code==ERROR_SUCCESS && count==0{return Ok(Vec::new());}
    if code!=ERROR_INSUFFICIENT_BUFFER || bytes==0 || bytes>1024*1024{return Err(error("Size native iSCSI session list",code));}
    let mut memory=vec![0u64;(bytes as usize).div_ceil(8)];
    let code=unsafe{GetIScsiSessionListW(&mut bytes,&mut count,memory.as_mut_ptr().cast())};
    if code!=ERROR_SUCCESS{return Err(error("Read native iSCSI sessions",code));}
    if count>256 || count as usize*size_of::<ISCSI_SESSION_INFOW>()>bytes as usize || bytes as usize>memory.len()*8{return Err("Truncated native session array".into());}
    let rows=unsafe{std::slice::from_raw_parts(memory.as_ptr().cast::<ISCSI_SESSION_INFOW>(),count as usize)};
    let mut result=Vec::new();
    for row in rows{
        let target=string_in(&memory,bytes as usize,row.TargetName)?;
        let mut connections=Vec::new();
        if row.ConnectionCount>16{return Err("Too many native iSCSI connections".into());}
        if row.ConnectionCount!=0{
            checked_offset(&memory,bytes as usize,row.Connections as usize,row.ConnectionCount as usize*size_of::<ISCSI_CONNECTION_INFOW>(),std::mem::align_of::<ISCSI_CONNECTION_INFOW>())?;
            for connection in unsafe{std::slice::from_raw_parts(row.Connections,row.ConnectionCount as usize)}{
                connections.push(Connection{target_address:string_in(&memory,bytes as usize,connection.TargetAddress)?,target_port:connection.TargetSocket});
            }
        }
        result.push(Session{id:row.SessionId.into(),target,connections});
    }
    Ok(result)
}
fn targets()->Result<Vec<String>,String>{
    let mut chars=0;
    let code=unsafe{ReportIScsiTargetsW(false,&mut chars,null_mut())};
    if code==ERROR_SUCCESS && chars==0{return Ok(Vec::new());}
    if code!=ERROR_INSUFFICIENT_BUFFER || chars==0 || chars>1024*1024{return Err(error("Size native target list",code));}
    let mut buffer=vec![0u16;chars as usize];
    let code=unsafe{ReportIScsiTargetsW(false,&mut chars,buffer.as_mut_ptr())};
    if code!=0{return Err(error("Read native target list",code));}
    if chars as usize>buffer.len(){return Err("Native target list overflow".into());}
    let mut result=Vec::new();let mut offset=0;
    while offset<chars as usize{
        let value=fixed_string(&buffer[offset..chars as usize])?;
        if value.is_empty(){return Ok(result);}
        offset+=value.encode_utf16().count()+1;result.push(value);
    }
    Err("Missing native target list terminator".into())
}
pub fn connect(target:&str)->Result<SessionId,String>{
    validate_target(target)?;
    if targets()?.iter().any(|value|value.eq_ignore_ascii_case(target)){return Err("Target already registered; existing state preserved".into());}
    let mut portal=ISCSI_TARGET_PORTALW{Socket:target_port(target),..Default::default()};
    let address=wide("127.0.0.1");portal.Address[..address.len()].copy_from_slice(&address);
    let mut group=ISCSI_TARGET_PORTAL_GROUPW{Count:1,Portals:[portal]};
    // SDK exposes these two bit values as string constants; the native field
    // is a numeric bitmap: AUTH_TYPE (0x80) | MAXIMUM_CONNECTIONS (0x04).
    let mut options=ISCSI_LOGIN_OPTIONS{Version:0,InformationSpecified:0x84,AuthType:ISCSI_NO_AUTH_TYPE,MaximumConnections:1,..Default::default()};
    let name=wide(target);
    let code=unsafe{AddIScsiStaticTargetW(name.as_ptr(),null(),0,false,null_mut(),&mut options,&mut group)};
    if code!=0{return Err(error("Add owned temporary static target",code));}
    let mut id=ISCSI_UNIQUE_SESSION_ID::default();let mut connection=ISCSI_UNIQUE_SESSION_ID::default();
    let code=unsafe{LoginIScsiTargetW(name.as_ptr(),false,null(),u32::MAX,&mut portal,0,null_mut(),&mut options,0,null(),false,&mut id,&mut connection)};
    if code!=0{
        let cleanup=unsafe{RemoveIScsiStaticTargetW(name.as_ptr())};
        return Err(format!("{}; static-target cleanup=0x{cleanup:08x}",error("Login owned loopback target",code)));
    }
    Ok(id.into())
}
pub fn devices(id:SessionId)->Result<Vec<Device>,String>{
    let mut native=id.native();let mut count=2;
    let mut rows=[ISCSI_DEVICE_ON_SESSIONW::default();2];
    let code=unsafe{GetDevicesForIScsiSessionW(&mut native,&mut count,rows.as_mut_ptr())};
    if code!=0{return Err(error("Query devices on owned session",code));}
    if count>2{return Err("Ambiguous native device count".into());}
    rows[..count as usize].iter().map(|row|Ok(Device{disk_number:row.StorageDeviceNumber.DeviceNumber,device_type:row.StorageDeviceNumber.DeviceType,legacy_name:fixed_string(&row.LegacyName)?,interface_name:fixed_string(&row.DeviceInterfaceName)?})).collect()
}
/// No implicit Drop logout: mounted filesystems must be drained by their owner.
pub fn disconnect(target:&str,id:SessionId)->Result<(),String>{
    validate_target(target)?;
    let matching:Vec<_>=sessions()?.into_iter().filter(|session|session.id==id).collect();
    if matching.len()!=1 || matching[0].target!=target || matching[0].connections.iter().any(|connection|connection.target_address!="127.0.0.1" || connection.target_port!=target_port(target)){return Err("Session identity/loopback endpoint changed; no logout sent".into());}
    let mut native=id.native();let code=unsafe{LogoutIScsiTarget(&mut native)};
    if code!=0{return Err(error("Logout owned loopback session",code));}Ok(())
}
pub fn remove_static_target(target:&str)->Result<(),String>{
    validate_target(target)?;
    if sessions()?.iter().any(|session|session.target==target){return Err("Owned target still has a session; preserved".into());}
    if !targets()?.iter().any(|value|value==target){return Ok(());}
    let code=unsafe{RemoveIScsiStaticTargetW(wide(target).as_ptr())};
    if code!=0{return Err(error("Remove owned temporary static target",code));}Ok(())
}
/// Conservative service restoration guard. Only sizes/counts are queried;
/// discovery portals and persistent logins are never added, removed or exposed.
pub fn has_control_resources()->Result<bool,String>{
    if !sessions()?.is_empty()||!targets()?.is_empty(){return Ok(true);}
    let mut count=0;let mut bytes=0;
    let code=unsafe{ReportIScsiSendTargetPortalsExW(&mut count,&mut bytes,null_mut())};
    if code==ERROR_INSUFFICIENT_BUFFER||count!=0||bytes!=0{return Ok(true);}
    if code!=0{return Err(error("Check other discovery portals",code));}
    let code=unsafe{ReportIScsiPersistentLoginsW(&mut count,null_mut(),&mut bytes)};
    if code==ERROR_INSUFFICIENT_BUFFER||count!=0||bytes!=0{return Ok(true);}
    if code!=0{return Err(error("Check other persistent initiator logins",code));}Ok(false)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn only_our_canonical_run_namespace_is_accepted(){
        assert!(target_name("0123456789abcdef0123456789abcdef").is_ok());
        for value in ["","1234","0123456789ABCDEF0123456789ABCDEF","../0123456789abcdef0123456789abc"]{assert!(target_name(value).is_err());}
        assert!(validate_target("iqn.remote:target").is_err());
        assert!(validate_target(&managed_target_name("0123456789abcdef0123456789abcdef").unwrap()).is_ok());
    }
    #[test]fn native_strings_and_arrays_cannot_escape_the_returned_buffer(){
        let memory=[0u64;8];let base=memory.as_ptr() as usize;
        assert_eq!(string_in(&memory,64,base as *const u16).unwrap(),"");
        for pointer in [base-2,base+1,base+64]{assert!(string_in(&memory,64,pointer as *const u16).is_err());}
        assert!(checked_offset(&memory,64,base+56,16,8).is_err());
        let no_zero=[u64::MAX;8];assert!(string_in(&no_zero,64,no_zero.as_ptr().cast()).is_err());
    }
}
