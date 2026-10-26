//! Closed SCM installation/control for the protected native physical service.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};
use superhdd_platform::native_package;
use superhdd_storage_control::{
    loopback::{Config, ROOT},
    recovery::{config_digest, Phase, RunJournal},
    windows::protected::ProtectedDir,
};
use windows_service::{
    service::*,
    service_manager::{ServiceManager, ServiceManagerAccess},
};
type Result<T> = std::result::Result<T, String>;
fn manager() -> Result<ServiceManager> {
    ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| e.to_string())
}
fn existing(manager: &ServiceManager, root: &Path) -> Result<Option<Service>> {
    let service = match manager.open_service(native_package::SERVICE, ServiceAccess::all()) {
        Ok(s) => s,
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => {
            return Ok(None)
        }
        Err(e) => return Err(e.to_string()),
    };
    let config = service.query_config().map_err(|e| e.to_string())?;
    let expected = format!(
        "\"{}\" --loopback-service",
        root.join("SuperHDDService.exe").display()
    );
    if !config
        .executable_path
        .to_string_lossy()
        .replace(r"\??\", "")
        .eq_ignore_ascii_case(&expected)
        || config.service_type != ServiceType::OWN_PROCESS
        || config.start_type != ServiceStartType::OnDemand
        || config
            .account_name
            .as_ref()
            .is_some_and(|a| !a.to_string_lossy().eq_ignore_ascii_case("LocalSystem"))
    {
        return Err("Existing native service belongs to another configuration; preserved".into());
    }
    Ok(Some(service))
}
fn wait(service: &Service, expected: ServiceState) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(120);
    loop {
        let state = service.query_status().map_err(|e| e.to_string())?;
        if state.current_state == expected {
            return Ok(());
        }
        if expected == ServiceState::Running && state.current_state == ServiceState::Stopped {
            return Err(format!(
                "Native service failed to start: {:?}",
                state.exit_code
            ));
        }
        if Instant::now() >= end {
            return Err(
                "Native service operation is still pending; protected ownership is preserved"
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn failure_policy(service: &Service, restart: bool) -> Result<()> {
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
            reboot_msg: Some(OsString::new()),
            command: Some(OsString::new()),
            actions: Some(vec![ServiceAction {
                action_type: if restart {
                    ServiceActionType::Restart
                } else {
                    ServiceActionType::None
                },
                delay: Duration::from_secs(2),
            }]),
        })
        .map_err(|e| e.to_string())
}
pub fn start(root: &Path, config: &Config) -> Result<()> {
    let manager = manager()?;
    let service = if let Some(service) = existing(&manager, root)? {
        service
    } else {
        manager
            .create_service(
                &ServiceInfo {
                    name: native_package::SERVICE.into(),
                    display_name: "Super HDD Local Cache".into(),
                    service_type: ServiceType::OWN_PROCESS,
                    start_type: ServiceStartType::OnDemand,
                    error_control: ServiceErrorControl::Normal,
                    executable_path: root.join("SuperHDDService.exe"),
                    launch_arguments: vec!["--loopback-service".into()],
                    dependencies: vec![ServiceDependency::Service("MSiSCSI".into())],
                    account_name: None,
                    account_password: None,
                },
                ServiceAccess::all(),
            )
            .map_err(|e| e.to_string())?
    };
    if service
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        == ServiceState::Running
    {
        return Ok(());
    }
    let owner = ProtectedDir::open(Path::new(ROOT))?;
    let mut manifest = owner.read_file("manifest.json")?;
    let mut bytes = Vec::new();
    (&mut manifest)
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Oversized native owner".into());
    }
    let mut journal = RunJournal::open(owner.file("lease.bin", false)?, config_digest(&bytes))?;
    if journal.record().phase == Phase::Stopped {
        journal.save(Phase::Prepared, Some(config.target()?))?;
    } else if journal.record().target.as_ref() != Some(&config.target()?) {
        return Err("Native owner differs from selected source".into());
    }
    drop(journal);
    drop(manifest);
    let initiator = manager
        .open_service(
            "MSiSCSI",
            ServiceAccess::QUERY_CONFIG
                | ServiceAccess::QUERY_STATUS
                | ServiceAccess::CHANGE_CONFIG
                | ServiceAccess::START,
        )
        .map_err(|e| e.to_string())?;
    let previous = initiator.query_config().map_err(|e| e.to_string())?;
    let was_running = initiator
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        == ServiceState::Running;
    if !owner.path().join("initiator-owner.json").exists() {
        let mut file = owner.file("initiator-owner.json", true)?;
        let bytes=serde_json::to_vec(&serde_json::json!({"schema":1,"wasRunning":was_running,"wasDisabled":previous.start_type==ServiceStartType::Disabled})).map_err(|e|e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    if previous.start_type == ServiceStartType::Disabled {
        // Change only startup policy; preserve Microsoft's executable, account,
        // dependencies and every other configured property.
        use windows_sys::Win32::System::Services::*;
        let raw_manager =
            unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if raw_manager.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let name: Vec<u16> = "MSiSCSI".encode_utf16().chain(Some(0)).collect();
        let raw = unsafe { OpenServiceW(raw_manager, name.as_ptr(), SERVICE_CHANGE_CONFIG) };
        if raw.is_null() {
            unsafe { CloseServiceHandle(raw_manager) };
            return Err(std::io::Error::last_os_error().to_string());
        }
        let ok = unsafe {
            ChangeServiceConfigW(
                raw,
                SERVICE_NO_CHANGE,
                SERVICE_DEMAND_START,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseServiceHandle(raw);
            CloseServiceHandle(raw_manager)
        };
        if ok == 0 {
            return Err(error.to_string());
        }
    }
    if !was_running {
        initiator.start(&[] as &[&str]).map_err(|e| e.to_string())?;
        wait(&initiator, ServiceState::Running)?;
    }
    failure_policy(&service, false)?;
    service.start(&[] as &[&str]).map_err(|e| e.to_string())?;
    if let Err(error) = wait(&service, ServiceState::Running) {
        if private_state()
            .ok()
            .is_some_and(|s| s["error"].as_str().unwrap_or("").contains("Source busy"))
        {
            return Err("runtimeSourceBusy".into());
        }
        return Err(error);
    }
    failure_policy(&service, true)
}
pub fn control(root: &Path, code: u32, stop: bool) -> Result<()> {
    if !matches!(code, 128..=131) || stop != (code == 131) {
        return Err("Unknown native service control".into());
    }
    let manager = manager()?;
    let service = existing(&manager, root)?.ok_or("Native service is not installed")?;
    if service
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        == ServiceState::Stopped
    {
        return if stop {
            Ok(())
        } else {
            Err("Native cache is stopped".into())
        };
    }
    if stop {
        failure_policy(&service, false)?;
    }
    let read_until = Instant::now() + Duration::from_secs(2);
    let before = loop {
        match private_state() {
            Ok(value) => break value,
            Err(e) if Instant::now() >= read_until => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let pid = before["servicePid"]
        .as_u64()
        .ok_or("Missing native service PID")?;
    if pid > u32::MAX as u64
        || service
            .query_status()
            .map_err(|e| e.to_string())?
            .process_id
            != Some(pid as u32)
    {
        return Err("Native service identity changed before control".into());
    }
    let raw = unsafe {
        windows_sys::Win32::System::Threading::OpenProcess(
            windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE,
            0,
            pid as u32,
        )
    };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    service
        .notify(UserEventCode::from_raw(code).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let end = Instant::now() + Duration::from_secs(45);
    loop {
        if stop
            && service
                .query_status()
                .map_err(|e| e.to_string())?
                .current_state
                == ServiceState::Stopped
        {
            if unsafe {
                windows_sys::Win32::System::Threading::WaitForSingleObject(
                    process.as_raw_handle(),
                    15000,
                )
            } != windows_sys::Win32::Foundation::WAIT_OBJECT_0
            {
                return Err("Stopped native service is still releasing its handles".into());
            }
            return Ok(());
        }
        if let Ok(state) = private_state() {
            if state["servicePid"] != before["servicePid"] {
                return Err("runtimeOperationPending".into());
            }
            if state["controlGeneration"].as_u64().unwrap_or(0)
                > before["controlGeneration"].as_u64().unwrap_or(0)
                && state["phase"] != "controlling"
            {
                if state["phase"] == "attention" {
                    return Err(if state["error"]
                        .as_str()
                        .unwrap_or("")
                        .contains("Source busy")
                    {
                        "runtimeSourceBusy"
                    } else {
                        "runtimeOperationFailed"
                    }
                    .into());
                }
                if !stop {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= end {
            return Err("runtimeOperationPending".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn private_state() -> Result<serde_json::Value> {
    let (config, _) = crate::loopback_ops::configuration()?.ok_or("Missing native owner")?;
    let run = ProtectedDir::open(&Path::new(ROOT).join(&config.run))?;
    let mut file = run.read_shared_file("service-state.json")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Oversized native service status".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["schema"] != 1
        || value["runId"] != config.run
        || value["servicePid"].as_u64().is_none_or(|pid| pid == 0)
    {
        return Err("Native service status differs from protected owner".into());
    }
    Ok(value)
}
pub fn remove_service(root: &Path) -> Result<()> {
    let manager = manager()?;
    if let Some(service) = existing(&manager, root)? {
        if service
            .query_status()
            .map_err(|e| e.to_string())?
            .current_state
            != ServiceState::Stopped
        {
            return Err("Stop the owned native service before removing its allocation".into());
        }
        failure_policy(&service, false)?;
        service.delete().map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn initiator_running() -> Result<bool> {
    let manager = manager()?;
    let service = manager
        .open_service("MSiSCSI", ServiceAccess::QUERY_STATUS)
        .map_err(|e| e.to_string())?;
    Ok(service
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        == ServiceState::Running)
}
pub fn restore_initiator(root: &ProtectedDir) -> Result<()> {
    if !root.path().join("initiator-owner.json").exists() {
        return Ok(());
    }
    let mut file = root.read_file("initiator-owner.json")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(1025)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 {
        return Err("Oversized initiator owner".into());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["schema"] != 1
        || value["wasRunning"].as_bool().is_none()
        || value["wasDisabled"].as_bool().is_none()
    {
        return Err("Invalid initiator restoration record".into());
    }
    if value["wasRunning"] == true {
        return Ok(());
    }
    let manager = manager()?;
    let service = manager
        .open_service(
            "MSiSCSI",
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::CHANGE_CONFIG,
        )
        .map_err(|e| e.to_string())?;
    if service
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        == ServiceState::Running
    {
        if superhdd_storage_control::iscsi::has_control_resources().unwrap_or(true) {
            return Ok(());
        }
        service.stop().map_err(|e| e.to_string())?;
        wait(&service, ServiceState::Stopped)?;
    }
    if value["wasDisabled"] == true {
        use windows_sys::Win32::System::Services::*;
        if unsafe {
            ChangeServiceConfigW(
                service.raw_handle(),
                SERVICE_NO_CHANGE,
                SERVICE_DISABLED,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}
