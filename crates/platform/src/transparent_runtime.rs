//! Desktop contract for the v7 storage filter. Exact qualified binaries are
//! required before UAC or enrollment; a development build cannot load a driver.
pub use crate::cache_runtime::{
    verify_file, verify_install_directory, Command, Operation, RuntimeState,
};
fn hash(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn qualified_driver_hash() -> Option<&'static str> {
    hash(option_env!("SUPERHDD_QUALIFIED_FILTER7_SHA256"))
}
fn service_hash() -> Option<&'static str> {
    hash(option_env!("SUPERHDD_QUALIFIED_FILTER7_SERVICE_SHA256"))
}
#[cfg(windows)]
pub fn install_root() -> Result<std::path::PathBuf, String> {
    Ok(crate::cache_runtime::install_root()?
        .parent()
        .ok_or("Invalid Program Files root")?
        .join("Super HDD Kernel Runtime"))
}
#[cfg(windows)]
fn package() -> Result<Vec<std::fs::File>, String> {
    let driver = qualified_driver_hash().ok_or("runtimeKernelDriverUnavailable")?;
    let helper = service_hash().ok_or("runtimeKernelDriverUnavailable")?;
    let root = install_root()?;
    Ok(vec![
        verify_install_directory(&root)?,
        verify_file(&root.join("SuperHDDCacheV7.sys"), driver)?,
        verify_file(&root.join("SuperHDDService.exe"), helper)?,
    ])
}
#[cfg(windows)]
pub fn available() -> bool {
    package().is_ok()
}
#[cfg(windows)]
pub fn state() -> RuntimeState {
    let (installed, scm) = crate::cache_runtime::service_state();
    let status = if scm == Some(windows_sys::Win32::System::Services::SERVICE_RUNNING) {
        crate::ipc::cache_status()
            .ok()
            .filter(|s| s["nativeVersion"] == 7)
    } else {
        None
    };
    let phase = status
        .as_ref()
        .and_then(|s| s["phase"].as_str())
        .unwrap_or(
            if scm == Some(windows_sys::Win32::System::Services::SERVICE_STOPPED) {
                "stopped"
            } else {
                "unknown"
            },
        )
        .to_string();
    let ready = package();
    let lab = status.as_ref().is_some_and(|s| s["vmTestBuild"] == true);
    let allowed = ready.is_ok() && !lab;
    RuntimeState {
        can_start: allowed && phase == "stopped",
        can_control: allowed && matches!(phase.as_str(), "active" | "paused" | "faulted"),
        can_remove: allowed && installed,
        reason: if lab {
            Some("runtimeTestBuild".into())
        } else {
            ready.err().or_else(|| {
                if phase == "unknown" {
                    Some("runtimeServiceUnavailable".into())
                } else {
                    None
                }
            })
        },
        native_version: Some(7),
        source_volume: status
            .as_ref()
            .and_then(|s| s["target"]["sourceVolume"].as_str())
            .map(str::to_string),
        request: status
            .as_ref()
            .and_then(|s| serde_json::from_value(s["request"].clone()).ok()),
        cache_used_bytes: None,
        phase,
    }
}
#[cfg(windows)]
pub fn execute(command: Command) -> Result<RuntimeState, String> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::*,
        System::Threading::*,
        UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
    };
    let encoded = command.encode()?;
    let _pins = package()?;
    let root = install_root()?;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let path = wide(
        root.join("SuperHDDService.exe")
            .to_str()
            .ok_or("Invalid runtime path")?,
    );
    let args = wide(&format!("--transparent-operation {encoded}"));
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: path.as_ptr(),
        lpParameters: args.as_ptr(),
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
    if unsafe { WaitForSingleObject(process.as_raw_handle(), 180000) } != WAIT_OBJECT_0 {
        return Err("runtimeOperationPending".into());
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if code != 0 {
        return Err(if code == 23 {
            "runtimeRestartRequired"
        } else {
            "runtimeOperationFailed"
        }
        .into());
    }
    Ok(state())
}
