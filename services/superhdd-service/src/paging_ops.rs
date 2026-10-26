//! Fixed, UAC-only setup/control. No executable, command or cache path comes
//! from the desktop request. Unsigned development packages fail before mutation.
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use superhdd_platform::{
    paging_runtime::{self as cache_runtime, Command, Operation},
    paging_cache::Context,
    paging_driver::Controller,
    ipc::{self, CacheAction},
};
use superhdd_storage_control::windows::{
    protected::{self, ProtectedDir},
    volume_device, volume_identity,
};
use windows_service::{
    service::*,
    service_manager::{ServiceManager, ServiceManagerAccess},
};
const OWNER: &str = "SuperHDDService";
const DRIVER: &str = "SuperHDDCacheV8";
pub(crate) struct TransparentOperationGuard {
    root: Option<ProtectedDir>,
    file: Option<std::fs::File>,
}
impl TransparentOperationGuard {
    pub(crate) fn acquire() -> Result<Self, String> {
        let root = ProtectedDir::create(Path::new(r"C:\ProgramData\SuperHDDSetup"))?;
        let file = root
            .file("operation.lock", true)
            .or_else(|_| root.file("operation.lock", false))?;
        if file.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err("Unexpected setup lock contents".into());
        }
        Ok(Self {
            root: Some(root),
            file: Some(file),
        })
    }
}
impl Drop for TransparentOperationGuard {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = protected::remove_file(file);
        }
        drop(self.root.take());
        let _ = std::fs::remove_dir(r"C:\ProgramData\SuperHDDSetup");
    }
}
fn info(root: &Path, kernel: bool, automatic: bool) -> ServiceInfo {
    ServiceInfo {
        name: if kernel { DRIVER } else { OWNER }.into(),
        display_name: if kernel {
            "Super HDD Cache Driver"
        } else {
            "Super HDD Service"
        }
        .into(),
        service_type: if kernel {
            ServiceType::KERNEL_DRIVER
        } else {
            ServiceType::OWN_PROCESS
        },
        start_type: if automatic {
            ServiceStartType::AutoStart
        } else {
            ServiceStartType::OnDemand
        },
        error_control: ServiceErrorControl::Normal,
        executable_path: root.join(if kernel {
            "SuperHDDCacheV8.sys"
        } else {
            "SuperHDDService.exe"
        }),
        launch_arguments: if kernel {
            vec![]
        } else {
            vec!["--managed-paging-service".into()]
        },
        dependencies: vec![],
        account_name: None,
        account_password: None,
    }
}
fn missing(error: &windows_service::Error) -> bool {
    matches!(error,windows_service::Error::Winapi(e) if e.raw_os_error()==Some(1060))
}
fn existing(
    manager: &ServiceManager,
    root: &Path,
    kernel: bool,
) -> Result<Option<Service>, String> {
    let service =
        match manager.open_service(if kernel { DRIVER } else { OWNER }, ServiceAccess::all()) {
            Ok(s) => s,
            Err(e) if missing(&e) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
    let c = service.query_config().map_err(|e| e.to_string())?;
    let path = root.join(if kernel {
        "SuperHDDCacheV8.sys"
    } else {
        "SuperHDDService.exe"
    });
    let expected = if kernel {
        path.to_string_lossy().into_owned()
    } else {
        format!("\"{}\" --managed-paging-service", path.display())
    };
    let actual = c.executable_path.to_string_lossy().replace(r"\??\", "");
    if !actual.eq_ignore_ascii_case(&expected)
        || c.service_type
            != if kernel {
                ServiceType::KERNEL_DRIVER
            } else {
                ServiceType::OWN_PROCESS
            }
        || c.account_name
            .as_ref()
            .is_some_and(|a| !a.to_string_lossy().eq_ignore_ascii_case("LocalSystem"))
    {
        return Err("A service with this name belongs to another installation; preserved".into());
    }
    Ok(Some(service))
}
fn ensure(manager: &ServiceManager, root: &Path, kernel: bool) -> Result<(Service, bool), String> {
    if let Some(s) = existing(manager, root, kernel)? {
        return Ok((s, false));
    }
    let service = manager
        .create_service(&info(root, kernel, false), ServiceAccess::all())
        .map_err(|e| e.to_string())?;
    Ok((service, true))
}
fn wait(service: &Service, expected: ServiceState) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let state = service.query_status().map_err(|e| e.to_string())?;
        if state.current_state == expected {
            return Ok(());
        }
        if expected == ServiceState::Running && state.current_state == ServiceState::Stopped {
            return Err(format!("Service start failed: {:?}", state.exit_code));
        }
        if Instant::now() >= deadline {
            return Err("Service transition is still pending".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn stop(service: &Service, root: &Path) -> Result<(), String> {
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
            reboot_msg: Some(OsString::new()),
            command: Some(OsString::new()),
            actions: Some(vec![ServiceAction {
                action_type: ServiceActionType::None,
                delay: Duration::ZERO,
            }]),
        })
        .map_err(|e| e.to_string())?;
    // Persist the user's stopped intent across reboot; crash recovery stays enabled.
    service
        .change_config(&info(root, false, false))
        .map_err(|e| e.to_string())?;
    if service
        .query_status()
        .map_err(|e| e.to_string())?
        .current_state
        != ServiceState::Stopped
    {
        service.stop().map_err(|e| e.to_string())?;
        wait(service, ServiceState::Stopped)?;
    }
    if superhdd_platform::paging_driver::try_telemetry()?.is_some_and(|s| s.cache.attached) {
        return Err("Kernel backing was not released; storage preserved".into());
    }
    Ok(())
}
fn enroll(context: &Context, install: bool) -> Result<(), String> {
    if context.isolated_lab.is_some() {
        return Err("Production enrollment cannot adopt a VM policy".into());
    }
    let current = volume_identity(&context.source.volume_guid)?;
    if current.serial != context.source.serial
        || current.extent_offset != context.source.extent_offset
        || current.extent_length != context.source.extent_length
    {
        return Err("Source changed before enrollment".into());
    }
    if !install && Controller::open(false)?.status()?.cache.attached {
        return Err("Stop the kernel session before removing its filter".into());
    }
    let root = ProtectedDir::create(Path::new(r"C:\ProgramData\SuperHDDSetup"))?;
    let mut script = root
        .file("enroll-paging-source.ps1", true)
        .or_else(|_| root.file("enroll-paging-source.ps1", false))?;
    let content = include_bytes!("../../../installer/enroll-paging-source.ps1");
    let length = script.metadata().map_err(|e| e.to_string())?.len();
    if length == 0 {
        script
            .write_all(content)
            .and_then(|_| script.sync_all())
            .map_err(|e| e.to_string())?;
    } else {
        if length != content.len() as u64 {
            return Err("Interrupted setup script differs; preserved".into());
        }
        let mut previous = vec![0; content.len()];
        script
            .read_exact(&mut previous)
            .map_err(|e| e.to_string())?;
        if previous.as_slice() != content {
            return Err("Interrupted setup script differs; preserved".into());
        }
    }
    let output = superhdd_platform::hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(root.path().join("enroll-paging-source.ps1"))
        .args([
            "-VolumeGuid",
            &current.volume_guid,
            "-DiskNumber",
            &current.disk_number.to_string(),
            "-NativeDevice",
            &volume_device(&current.volume_guid)?,
            "-Action",
            if install { "Install" } else { "Remove" },
        ])
        .output()
        .map_err(|e| e.to_string());
    let cleanup = protected::remove_file(script);
    drop(root);
    let _ = std::fs::remove_dir(r"C:\ProgramData\SuperHDDSetup");
    cleanup?;
    let output = output?;
    if !output.status.success() {
        if output.status.code()==Some(10){return Err("runtimeRestartRequired".into());}
        return Err(format!(
            "Source enrollment needs attention (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        ));
    }
    Ok(())
}
fn recovery(service: &Service) -> Result<(), String> {
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
            reboot_msg: Some(OsString::new()),
            command: Some(OsString::new()),
            actions: Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(1),
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_secs(5),
                },
                ServiceAction {
                    action_type: ServiceActionType::None,
                    delay: Duration::ZERO,
                },
            ]),
        })
        .map_err(|e| e.to_string())?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| e.to_string())
}
pub fn run(command: Command) -> Result<(), String> {
    command.validate()?;
    let expected = cache_runtime::qualified_driver_hash().ok_or("runtimeReleasePending")?;
    let root = cache_runtime::install_root()?;
    let _directory = cache_runtime::verify_install_directory(&root)?;
    let current = std::fs::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let installed =
        std::fs::canonicalize(root.join("SuperHDDService.exe")).map_err(|e| e.to_string())?;
    if current != installed {
        return Err("Run the installed runtime helper".into());
    }
    let _helper = cache_runtime::verify_file(&installed, "")?;
    let _driver = cache_runtime::verify_file(&root.join("SuperHDDCacheV8.sys"), expected)?;
    let _operation = TransparentOperationGuard::acquire()?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| e.to_string())?;
    match command.operation {
        Operation::Pause | Operation::Resume | Operation::Clear => {
            existing(&manager, &root, false)?.ok_or("runtimeNotInstalled")?;
            ipc::cache_action(match command.operation {
                Operation::Pause => CacheAction::Pause,
                Operation::Resume => CacheAction::Resume,
                _ => CacheAction::Clear,
            })?;
            Ok(())
        }
        Operation::Stop => stop(
            &existing(&manager, &root, false)?.ok_or("runtimeNotInstalled")?,
            &root,
        ),
        Operation::Remove => {
            // After a reboot which removes the source's per-device filter,
            // load the verified control endpoint to prove that no cache is live.
            let (kernel,_)=ensure(&manager,&root,true)?;
            if kernel.query_status().map_err(|e|e.to_string())?.current_state==ServiceState::Stopped {
                kernel.start::<&str>(&[]).map_err(|e|e.to_string())?;wait(&kernel,ServiceState::Running)?;
            }
            let service = existing(&manager, &root, false)?;
            if let Some(s) = &service {
                stop(s, &root)?;
            }
            if let Some(config) = crate::paging_host::configuration()? {
                enroll(&config.context, false)?;
                crate::paging_host::remove_production()?;
            } else if superhdd_platform::paging_driver::try_telemetry()?
                .is_some_and(|s| s.cache.attached)
            {
                return Err("An unowned kernel session remains".into());
            }
            let refs=superhdd_platform::hidden_command("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",
                "$ErrorActionPreference='Stop';$refs=@(Get-PnpDevice -Class Volume|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data -contains 'SuperHDDCacheV8'});if($refs.Count){exit 11}"])
                .output().map_err(|e|e.to_string())?;
            if !refs.status.success() {
                return Err(
                    "A volume still references this driver; runtime files preserved".into(),
                );
            }
            if let Some(s) = service {
                s.delete().map_err(|e| e.to_string())?;
            }
            if let Some(driver) = existing(&manager, &root, true)? {
                driver.delete().map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Operation::Start => {
            let request = command.request.ok_or("Missing source selection")?;
            let plan =
                superhdd_platform::plan_cache(&superhdd_platform::discover()?, request.clone())?;
            if plan.reasons.iter().any(|r| r == "encryptionUnknown") {
                return Err("encryptionUnknown".into());
            }
            let parent = volume_identity(&request.ssd_letter)?;
            let cache_root = PathBuf::from(format!("{}SuperHDDCache", parent.volume_guid));
            let context = Context::capture(&request.source_letter, &cache_root, None)?;
            let old = crate::paging_host::configuration()?;
            if old.as_ref().is_some_and(|c| {
                c.context.source.volume_guid != context.source.volume_guid
                    || c.container_root != cache_root
                    || c.budget_bytes != request.capacity_bytes
            }) {
                return Err(
                    "Stop and remove the previous cache before changing its source or budget"
                        .into(),
                );
            }
            if let Some(s) = existing(&manager, &root, false)? {
                if s.query_status().map_err(|e| e.to_string())?.current_state
                    == ServiceState::Running
                {
                    return Ok(());
                }
            }
            let (driver, driver_new) = ensure(&manager, &root, true)?;
            if driver
                .query_status()
                .map_err(|e| e.to_string())?
                .current_state
                == ServiceState::Stopped
            {
                driver.start::<&str>(&[]).map_err(|e| e.to_string())?;
                wait(&driver, ServiceState::Running)?;
            }
            if Controller::open(false)?.status()?.cache.attached {
                return Err("Another kernel session is active; preserved".into());
            }
            let result = (|| -> Result<(), String> {
                if old.is_none() {
                    crate::paging_host::prepare(crate::paging_host::Config {
                        schema: 8,
                        context: context.clone(),
                        container_root: cache_root,
                        budget_bytes: request.capacity_bytes,
                        request:Some(request.clone()),
                    })?;
                }
                // Persist start intent before per-device enrollment. A paging
                // source loads its filter at the next normal reboot.
                let (service, _) = ensure(&manager, &root, false)?;
                recovery(&service)?;
                service
                    .change_config(&info(&root, false, true))
                    .map_err(|e| e.to_string())?;
                if Controller::open(false)?.status()?.volume_count==0 {enroll(&context, true)?;}
                service.start::<&str>(&[]).map_err(|e| e.to_string())?;
                wait(&service, ServiceState::Running)
            })();
            if result.as_ref().is_err_and(|e|e=="runtimeRestartRequired"){return result;}
            if result.is_err()
                && Controller::open(false)
                    .is_ok_and(|c| c.status().is_ok_and(|s| !s.cache.attached))
            {
                // Preserve an existing configuration and its source enrollment.
                // For fresh setup, remove only the exact new filter after safe stop.
                if old.is_none() {
                    if let Some(s) = existing(&manager, &root, false)? {
                        let _ = stop(&s, &root);
                    }
                    if enroll(&context, false).is_ok() {
                        if crate::paging_host::configuration()?.is_some() {
                            let _ = crate::paging_host::remove_production();
                        }
                        if let Some(s) = existing(&manager, &root, false)? {
                            let _ = s.delete();
                        }
                        if driver_new {
                            let _ = driver.delete();
                        }
                    }
                }
            }
            result
        }
    }
}

