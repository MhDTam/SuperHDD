//! Desktop adapter for the separately installed, driver-free native backend.
//! Package ACL/hash verification and SCM-authenticated read-only IPC are required.
use crate::{
    cache_runtime::{Command, RuntimeState},
    native_package,
};
use windows_sys::Win32::System::Services::*;
pub fn available() -> bool {
    native_package::verify().is_ok_and(|p| !p.manifest.lab_only)
}
fn service_state() -> Result<Option<u32>, String> {
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let name: Vec<u16> = native_package::SERVICE
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let service = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            let error = std::io::Error::last_os_error();
            CloseServiceHandle(manager);
            return if error.raw_os_error() == Some(1060) {
                Ok(None)
            } else {
                Err(error.to_string())
            };
        }
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        );
        let error = std::io::Error::last_os_error();
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        if ok == 0 {
            return Err(error.to_string());
        }
        Ok(Some(status.dwCurrentState))
    }
}
pub fn state() -> Option<RuntimeState> {
    let package = native_package::verify().ok()?;
    if package.manifest.lab_only {
        return None;
    }
    let (phase, installed, reason) = match service_state() {
        Ok(Some(SERVICE_RUNNING)) => {
            if let Ok(value) = crate::ipc::native_status(false) {
                if value["labOnly"] == false && value["nativeVersion"] == 6 {
                    if let Ok(state) = serde_json::from_value::<RuntimeState>(value) {
                        return Some(state);
                    }
                }
            }
            ("unknown", true, Some("runtimeServiceUnavailable".into()))
        }
        Ok(Some(SERVICE_STOPPED)) => ("stopped", true, None),
        Ok(None) => ("stopped", false, None),
        Ok(Some(_)) => ("unknown", true, Some("runtimeOperationPending".into())),
        Err(_) => ("unknown", false, Some("runtimeServiceUnavailable".into())),
    };
    Some(RuntimeState {
        phase: phase.into(),
        can_start: phase == "stopped",
        can_control: false,
        can_remove: installed && phase == "stopped",
        reason,
        native_version: Some(6),
        source_volume: None,
        request: None,
        cache_used_bytes: None,
    })
}
pub fn execute(command: Command) -> Result<RuntimeState, String> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::*,
        System::Threading::*,
        UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
    };
    let encoded = command.encode()?;
    let package = native_package::verify()?;
    if package.manifest.lab_only {
        return Err("runtimeTestBuild".into());
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let path = wide(
        package
            .root
            .join("SuperHDDService.exe")
            .to_str()
            .ok_or("Invalid native package path")?,
    );
    let arguments = wide(&format!("--loopback-operation {encoded}"));
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: path.as_ptr(),
        lpParameters: arguments.as_ptr(),
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
        return Err("Native helper process unavailable".into());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 180000) } != WAIT_OBJECT_0 {
        return Err("runtimeOperationPending".into());
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if code != 0 {
        return Err(match code {
            20 => "runtimeSourceBusy",
            21 => "selectedScopeUnavailable",
            22 => "runtimeOperationPending",
            _ => "runtimeOperationFailed",
        }
        .into());
    }
    state().ok_or("runtimeServiceUnavailable".into())
}
