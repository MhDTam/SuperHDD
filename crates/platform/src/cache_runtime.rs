//! Closed desktop/UAC contract. A qualified release embeds the exact Microsoft-
//! signed driver hash and the helper hash; development builds cannot activate it.
use crate::CacheRequest;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Start,
    Stop,
    Pause,
    Resume,
    Clear,
    Remove,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub operation: Operation,
    pub request: Option<CacheRequest>,
}
impl Command {
    pub fn validate(&self) -> Result<(), String> {
        if matches!(self.operation, Operation::Start) != self.request.is_some() {
            return Err("Invalid cache command shape".into());
        }
        if let Some(r) = &self.request {
            // The volume driver cannot enforce a file/folder allowlist. Never
            // silently turn a selected-only request into whole-volume caching.
            if r.scope.is_some() {
                return Err("selectedScopeUnavailable".into());
            }
            let letter = |v: &str| {
                v.len() == 2 && v.as_bytes()[0].is_ascii_alphabetic() && v.as_bytes()[1] == b':'
            };
            if !letter(&r.source_letter)
                || !letter(&r.ssd_letter)
                || r.source_disk_id.is_empty()
                || r.ssd_disk_id.is_empty()
                || r.source_disk_id.len() > 256
                || r.ssd_disk_id.len() > 256
                || r.source_disk_id == r.ssd_disk_id
                || r.capacity_bytes < 1024u64.pow(3)
                || r.capacity_bytes > superhdd_storage_control::MAX_BUDGET_BYTES
                || !r.capacity_bytes.is_multiple_of(65536)
            {
                return Err("Invalid cache request identities or budget".into());
            }
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if bytes.len() > 4096 {
            return Err("Cache request too large".into());
        }
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    pub fn decode(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || value.len() > 8192
            || !value.len().is_multiple_of(2)
            || !value.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Malformed cache operation".into());
        }
        let bytes: Result<Vec<_>, _> = (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
            .collect();
        let command: Self = serde_json::from_slice(&bytes.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        command.validate()?;
        Ok(command)
    }
}
fn valid_hash(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn qualified_driver_hash() -> Option<&'static str> {
    valid_hash(option_env!("SUPERHDD_QUALIFIED_DRIVER_SHA256"))
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub phase: String,
    pub can_start: bool,
    pub can_control: bool,
    pub can_remove: bool,
    pub reason: Option<String>,
    pub native_version: Option<u32>,
    pub source_volume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<CacheRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_used_bytes: Option<u64>,
}
#[cfg(windows)]
pub fn install_root() -> Result<std::path::PathBuf, String> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramFiles, SHGetKnownFolderPath},
    };
    let mut pointer = std::ptr::null_mut();
    if unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_ProgramFiles,
            0,
            std::ptr::null_mut(),
            &mut pointer,
        )
    } < 0
        || pointer.is_null()
    {
        return Err("runtimeNotInstalled".into());
    }
    let mut count = 0;
    unsafe {
        while count < 32768 && *pointer.add(count) != 0 {
            count += 1;
        }
    }
    let path = if count == 32768 {
        Err("Invalid Program Files path".to_string())
    } else {
        String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, count) })
            .map_err(|e| e.to_string())
    };
    unsafe { CoTaskMemFree(pointer.cast()) };
    Ok(std::path::PathBuf::from(path?).join("Super HDD Runtime"))
}
#[cfg(windows)]
pub fn verify_install_directory(path: &std::path::Path) -> Result<std::fs::File, String> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "runtimeNotInstalled")?;
    let flags = file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_attributes();
    if flags & FILE_ATTRIBUTE_DIRECTORY == 0
        || flags
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_ENCRYPTED | FILE_ATTRIBUTE_COMPRESSED)
            != 0
    {
        return Err("runtimePermissionsUnsafe".into());
    }
    let mut raw = std::ptr::null_mut();
    if unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut raw,
        )
    } != 0
    {
        return Err("runtimePermissionsUnsafe".into());
    }
    let result = (|| -> Result<(), String> {
        let mut owner = std::ptr::null_mut();
        let mut d = 0;
        let mut control = 0;
        let mut revision = 0;
        let mut present = 0;
        let mut acl = std::ptr::null_mut();
        if unsafe { GetSecurityDescriptorOwner(raw, &mut owner, &mut d) } == 0
            || owner.is_null()
            || unsafe {
                IsWellKnownSid(owner, WinBuiltinAdministratorsSid) == 0
                    && IsWellKnownSid(owner, WinLocalSystemSid) == 0
            }
            || unsafe { GetSecurityDescriptorControl(raw, &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
            || unsafe { GetSecurityDescriptorDacl(raw, &mut present, &mut acl, &mut d) } == 0
            || present == 0
            || acl.is_null()
        {
            return Err("runtimePermissionsUnsafe".into());
        }
        let mut system = false;
        let mut admin = false;
        for index in 0..unsafe { (*acl).AceCount } {
            let mut ace = std::ptr::null_mut();
            if unsafe { GetAce(acl, u32::from(index), &mut ace) } == 0 {
                return Err("runtimePermissionsUnsafe".into());
            }
            let a = ace.cast::<ACCESS_ALLOWED_ACE>();
            if unsafe { (*a).Header.AceType } != 0
                || unsafe { (*a).Header.AceFlags } & INHERIT_ONLY_ACE as u8 != 0
            {
                return Err("runtimePermissionsUnsafe".into());
            }
            let sid = unsafe { std::ptr::addr_of_mut!((*a).SidStart).cast() };
            let mask = unsafe { (*a).Mask };
            if unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } != 0 && mask == FILE_ALL_ACCESS {
                system = true;
            } else if unsafe { IsWellKnownSid(sid, WinBuiltinAdministratorsSid) } != 0
                && mask == FILE_ALL_ACCESS
            {
                admin = true;
            } else if unsafe { IsWellKnownSid(sid, WinBuiltinUsersSid) } == 0
                || mask != (FILE_GENERIC_READ | FILE_GENERIC_EXECUTE)
            {
                return Err("runtimePermissionsUnsafe".into());
            }
        }
        if !system || !admin {
            return Err("runtimePermissionsUnsafe".into());
        }
        Ok(())
    })();
    unsafe { LocalFree(raw) };
    result?;
    Ok(file)
}
#[cfg(windows)]
pub fn verify_file(path: &std::path::Path, expected: &str) -> Result<std::fs::File, String> {
    use sha2::{Digest, Sha256};
    use std::{
        io::Read,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
    };
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "runtimeNotInstalled")?;
    let flags = f.metadata().map_err(|e| e.to_string())?.file_attributes();
    if flags & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_ENCRYPTED)
        != 0
    {
        return Err("runtimePackageChanged".into());
    }
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut identity) } == 0
        || identity.nNumberOfLinks != 1
    {
        return Err("runtimePackageChanged".into());
    }
    let mut raw = std::ptr::null_mut();
    let code = unsafe {
        GetSecurityInfo(
            f.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err("Cannot inspect installed runtime permissions".into());
    }
    let result = (|| -> Result<(), String> {
        let mut owner = std::ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut acl = std::ptr::null_mut();
        let trusted = |s: PSID| {
            !s.is_null()
                && unsafe {
                    IsWellKnownSid(s, WinLocalSystemSid) != 0
                        || IsWellKnownSid(s, WinBuiltinAdministratorsSid) != 0
                }
        };
        if unsafe { GetSecurityDescriptorOwner(raw, &mut owner, &mut defaulted) } == 0
            || !trusted(owner)
            || unsafe { GetSecurityDescriptorDacl(raw, &mut present, &mut acl, &mut defaulted) }
                == 0
            || present == 0
            || acl.is_null()
        {
            return Err("runtimePermissionsUnsafe".into());
        }
        let write = FILE_WRITE_DATA
            | FILE_APPEND_DATA
            | FILE_WRITE_EA
            | FILE_WRITE_ATTRIBUTES
            | FILE_DELETE_CHILD
            | DELETE
            | WRITE_DAC
            | WRITE_OWNER
            | 0x40000000
            | 0x10000000;
        for index in 0..unsafe { (*acl).AceCount } {
            let mut ace = std::ptr::null_mut();
            if unsafe { GetAce(acl, u32::from(index), &mut ace) } == 0 {
                return Err("runtimePermissionsUnsafe".into());
            }
            let a = ace.cast::<ACCESS_ALLOWED_ACE>();
            let kind = unsafe { (*a).Header.AceType };
            if kind == 1 {
                continue;
            }
            if kind != 0 {
                return Err("runtimePermissionsUnsafe".into());
            }
            let sid = unsafe { std::ptr::addr_of_mut!((*a).SidStart).cast() };
            if !trusted(sid) && unsafe { (*a).Mask } & write != 0 {
                return Err("runtimePermissionsUnsafe".into());
            }
        }
        Ok(())
    })();
    unsafe { LocalFree(raw) };
    result?;
    let mut hasher = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        let n = f.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    if !expected.is_empty() && !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected) {
        return Err("runtimePackageChanged".into());
    }
    Ok(f)
}
#[cfg(windows)]
pub(crate) fn service_state() -> (bool, Option<u32>) {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_SERVICE_DOES_NOT_EXIST},
        System::Services::*,
    };
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return (false, None);
        }
        let name: Vec<u16> = "SuperHDDService".encode_utf16().chain(Some(0)).collect();
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            let missing = GetLastError() == ERROR_SERVICE_DOES_NOT_EXIST;
            CloseServiceHandle(manager);
            return (false, missing.then_some(SERVICE_STOPPED));
        }
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        ) != 0;
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        (true, ok.then_some(status.dwCurrentState))
    }
}
#[cfg(windows)]
pub fn state() -> RuntimeState {
    if let Some(state) = crate::native_runtime::state() {
        if matches!(state.phase.as_str(), "active" | "paused" | "faulted") {
            return state;
        }
    }
    let prior = crate::transparent_runtime::state();
    if matches!(prior.phase.as_str(), "active" | "paused" | "faulted") {
        return prior;
    }
    crate::paging_runtime::state()
}
#[cfg(windows)]
pub fn state_legacy_v5() -> RuntimeState {
    let status = crate::ipc::cache_status().ok();
    let (installed, scm_state) = service_state();
    let phase = status
        .as_ref()
        .and_then(|s| s["phase"].as_str())
        .unwrap_or(
            if scm_state == Some(windows_sys::Win32::System::Services::SERVICE_STOPPED) {
                "stopped"
            } else {
                "unknown"
            },
        )
        .to_string();
    let lab = status.as_ref().is_some_and(|s| s["vmTestBuild"] == true);
    let ready = (|| -> Result<(), String> {
        let driver = qualified_driver_hash().ok_or("runtimeReleasePending")?;
        let helper = valid_hash(option_env!("SUPERHDD_QUALIFIED_SERVICE_SHA256"))
            .ok_or("runtimeReleasePending")?;
        let root = install_root()?;
        let _directory = verify_install_directory(&root)?;
        let _driver = verify_file(&root.join("SuperHDDCacheDriver.sys"), driver)?;
        let _helper = verify_file(&root.join("SuperHDDService.exe"), helper)?;
        if lab {
            return Err("runtimeTestBuild".into());
        }
        Ok(())
    })();
    RuntimeState {
        request: None,
        cache_used_bytes: None,
        can_start: ready.is_ok() && phase == "stopped",
        can_control: ready.is_ok() && matches!(phase.as_str(), "active" | "paused" | "faulted"),
        can_remove: ready.is_ok() && installed,
        reason: ready.err().or_else(|| match phase.as_str() {
            "unknown" => Some("runtimeServiceUnavailable".into()),
            "faulted" => Some("runtimeSessionFaulted".into()),
            _ => None,
        }),
        native_version: status.as_ref().map(|_| 5),
        source_volume: status
            .as_ref()
            .and_then(|s| s["target"]["sourceVolume"].as_str())
            .map(str::to_string),
        phase,
    }
}
#[cfg(windows)]
pub fn execute(command: Command) -> Result<RuntimeState, String> {
    if !matches!(command.operation, Operation::Start)
        && crate::native_runtime::state()
            .is_some_and(|s| matches!(s.phase.as_str(), "active" | "paused" | "faulted"))
    {
        return crate::native_runtime::execute(command);
    }
    if !matches!(command.operation, Operation::Start)
        && matches!(
            crate::transparent_runtime::state().phase.as_str(),
            "active" | "paused" | "faulted"
        )
    {
        return crate::transparent_runtime::execute(command);
    }
    crate::paging_runtime::execute(command)
}
#[cfg(windows)]
pub fn execute_legacy_v5(command: Command) -> Result<RuntimeState, String> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::*,
        System::Threading::*,
        UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
    };
    let encoded = command.encode()?;
    let driver = qualified_driver_hash().ok_or("runtimeReleasePending")?;
    let helper = valid_hash(option_env!("SUPERHDD_QUALIFIED_SERVICE_SHA256"))
        .ok_or("runtimeReleasePending")?;
    let root = install_root()?;
    let _directory = verify_install_directory(&root)?;
    let _driver = verify_file(&root.join("SuperHDDCacheDriver.sys"), driver)?;
    let _helper = verify_file(&root.join("SuperHDDService.exe"), helper)?;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let path = wide(
        root.join("SuperHDDService.exe")
            .to_str()
            .ok_or("Invalid runtime path")?,
    );
    let parameters = wide(&format!("--cache-operation {encoded}"));
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: path.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(if unsafe { GetLastError() } == ERROR_CANCELLED {
            "runtimeCancelled".into()
        } else {
            std::io::Error::last_os_error().to_string()
        });
    }
    if info.hProcess.is_null() {
        return Err("Runtime helper process unavailable".into());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 120000) } != WAIT_OBJECT_0 {
        return Err("runtimeOperationPending".into());
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 || code != 0 {
        return Err("runtimeOperationFailed".into());
    }
    Ok(state_legacy_v5())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uac_payload_is_closed_bounded_and_rejects_unknown_fields() {
        let command = Command {
            operation: Operation::Pause,
            request: None,
        };
        let encoded = command.encode().unwrap();
        assert!(matches!(
            Command::decode(&encoded).unwrap().operation,
            Operation::Pause
        ));
        for value in ["", "xx", "0", "ff", &"00".repeat(4097)] {
            assert!(Command::decode(value).is_err());
        }
        assert!(serde_json::from_str::<Command>(
            r#"{"operation":"pause","request":null,"script":"anything"}"#
        )
        .is_err());
        assert!(Command {
            operation: Operation::Start,
            request: None
        }
        .validate()
        .is_err());
    }
}
