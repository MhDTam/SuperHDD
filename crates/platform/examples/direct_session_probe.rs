//! Real LocalSystem SCM lifecycle and automatic service-crash reconciliation.
//! Refuses every environment except the provisioned, network-isolated lab VM.
#[cfg(windows)]
mod proof {
    use serde::Deserialize;
    use serde_json::{json, Value};
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };
    use superhdd_platform::{direct_driver::Controller, ipc};
    use superhdd_storage_control::{
        lab::LabIdentity,
        windows::{protected::ProtectedDir, volume_identity, OwnedVhd, VerifiedLab},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Security::*,
        System::{Services::*, Threading::*},
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    const STATE: &str = r"C:\ProgramData\SuperHDDDirect";
    const NAME: &str = "SuperHDDService";
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    fn error(message: &str) -> Box<dyn std::error::Error> {
        format!("{message}: {}", std::io::Error::last_os_error()).into()
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Service {
        manager: SC_HANDLE,
        service: SC_HANDLE,
    }
    impl Service {
        fn install(tool: &Path) -> Result<Self> {
            let manager = unsafe {
                OpenSCManagerW(
                    null(),
                    null(),
                    SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
                )
            };
            if manager.is_null() {
                return Err(error("Open SCM"));
            }
            let command = format!("\"{}\" --managed-direct-service", tool.display());
            let service = unsafe {
                CreateServiceW(
                    manager,
                    wide(NAME).as_ptr(),
                    wide("Super HDD protected recovery VM test").as_ptr(),
                    SERVICE_ALL_ACCESS,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_DEMAND_START,
                    SERVICE_ERROR_NORMAL,
                    wide(&command).as_ptr(),
                    null(),
                    null_mut(),
                    null(),
                    null(),
                    null(),
                )
            };
            if service.is_null() {
                unsafe {
                    CloseServiceHandle(manager);
                }
                return Err(error("Create owned VM service"));
            }
            Ok(Self { manager, service })
        }
        fn actions(&self, restart: bool) -> Result<()> {
            let mut actions = [
                SC_ACTION {
                    Type: if restart {
                        SC_ACTION_RESTART
                    } else {
                        SC_ACTION_NONE
                    },
                    Delay: 1000,
                },
                SC_ACTION {
                    Type: if restart {
                        SC_ACTION_RESTART
                    } else {
                        SC_ACTION_NONE
                    },
                    Delay: 1000,
                },
                SC_ACTION {
                    Type: SC_ACTION_NONE,
                    Delay: 0,
                },
            ];
            let info = SERVICE_FAILURE_ACTIONSW {
                dwResetPeriod: 60,
                lpRebootMsg: null_mut(),
                lpCommand: null_mut(),
                cActions: actions.len() as u32,
                lpsaActions: actions.as_mut_ptr(),
            };
            if unsafe {
                ChangeServiceConfig2W(
                    self.service,
                    SERVICE_CONFIG_FAILURE_ACTIONS,
                    (&info as *const SERVICE_FAILURE_ACTIONSW).cast(),
                )
            } == 0
            {
                return Err(error("Configure owned service recovery actions"));
            }
            Ok(())
        }
        fn status(&self) -> Result<SERVICE_STATUS_PROCESS> {
            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes = 0;
            if unsafe {
                QueryServiceStatusEx(
                    self.service,
                    SC_STATUS_PROCESS_INFO,
                    (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                    size_of::<SERVICE_STATUS_PROCESS>() as u32,
                    &mut bytes,
                )
            } == 0
            {
                return Err(error("Query owned service"));
            }
            Ok(status)
        }
        fn start(&self) -> Result<()> {
            if unsafe { StartServiceW(self.service, 0, null()) } == 0 {
                return Err(error("Start owned service"));
            }
            Ok(())
        }
        fn wait(&self, state: u32, previous_pid: u32) -> Result<SERVICE_STATUS_PROCESS> {
            let deadline = Instant::now() + Duration::from_secs(50);
            loop {
                let s = self.status()?;
                if s.dwCurrentState == state
                    && (state != SERVICE_RUNNING
                        || s.dwProcessId != 0 && s.dwProcessId != previous_pid)
                {
                    return Ok(s);
                }
                if state == SERVICE_RUNNING
                    && previous_pid == 0
                    && s.dwCurrentState == SERVICE_STOPPED
                    && s.dwWin32ExitCode != 0
                {
                    return Err(format!(
                        "Managed SCM startup failed: win32 {}, service {}",
                        s.dwWin32ExitCode, s.dwServiceSpecificExitCode
                    )
                    .into());
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "SCM wait timed out: state {}, pid {}, win32 {}, service {}",
                        s.dwCurrentState,
                        s.dwProcessId,
                        s.dwWin32ExitCode,
                        s.dwServiceSpecificExitCode
                    )
                    .into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        fn stop(&self) -> Result<()> {
            let s = self.status()?;
            if s.dwCurrentState == SERVICE_STOPPED {
                return Ok(());
            }
            let mut status = SERVICE_STATUS::default();
            if unsafe { ControlService(self.service, SERVICE_CONTROL_STOP, &mut status) } == 0 {
                return Err(error("Stop owned service"));
            }
            self.wait(SERVICE_STOPPED, 0)?;
            Ok(())
        }
        fn kill(&self, tool: &Path) -> Result<u32> {
            let s = self.status()?;
            if s.dwCurrentState != SERVICE_RUNNING || s.dwProcessId == 0 {
                return Err("No running owned SCM process".into());
            }
            let process = unsafe {
                OpenProcess(
                    PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    s.dwProcessId,
                )
            };
            if process.is_null() {
                return Err(error("Open owned SCM process"));
            }
            let process = Handle(process);
            let mut image = vec![0u16; 32768];
            let mut length = image.len() as u32;
            if unsafe { QueryFullProcessImageNameW(process.0, 0, image.as_mut_ptr(), &mut length) }
                == 0
            {
                return Err(error("Verify owned SCM image"));
            }
            let image = String::from_utf16(&image[..length as usize])?;
            if !image.eq_ignore_ascii_case(tool.to_str().ok_or("Invalid service path")?)
                || self.status()?.dwProcessId != s.dwProcessId
            {
                return Err("SCM PID/image ownership changed; no kill sent".into());
            }
            if unsafe { TerminateProcess(process.0, 91) } == 0 {
                return Err(error("Terminate exact owned service process"));
            }
            if unsafe { WaitForSingleObject(process.0, 10000) } != WAIT_OBJECT_0 {
                return Err("Owned process failed to exit".into());
            }
            Ok(s.dwProcessId)
        }
    }
    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.actions(false);
            let _ = self.stop();
            unsafe {
                DeleteService(self.service);
                CloseServiceHandle(self.service);
                CloseServiceHandle(self.manager);
            }
        }
    }
    fn cli(tool: &Path, command: &str, value: &str) -> Result<()> {
        let result =
            superhdd_platform::hidden_command(tool.to_str().ok_or("Invalid service path")?)
                .args([command, value])
                .output()?;
        if !result.status.success() {
            return Err(format!("{command}: {}", String::from_utf8_lossy(&result.stderr)).into());
        }
        Ok(())
    }
    fn state(pid: u32, phase: &str) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(bytes) = std::fs::read(Path::new(STATE).join("status.json")) {
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                    if value["phase"] == phase
                        && (pid == 0 || value["pid"].as_u64() == Some(u64::from(pid)))
                    {
                        return Ok(value);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(format!("Protected status did not reach {phase} for PID {pid}").into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[repr(align(4096))]
    struct Block([u8; 65536]);
    fn pattern(block: &mut Block, index: usize, revision: usize) {
        for (i, b) in block.0.iter_mut().enumerate() {
            *b = ((i * 31 + i / 13 + index * 19 + revision * 71) % 251) as u8;
        }
    }
    fn reads(file: &mut File, revision: usize) -> Result<()> {
        let mut wanted = Box::new(Block([0; 65536]));
        let mut actual = Box::new(Block([0; 65536]));
        file.seek(SeekFrom::Start(0))?;
        for index in 0..4 {
            pattern(&mut wanted, index, if index == 0 { revision } else { 0 });
            file.read_exact(&mut actual.0)?;
            if actual.0 != wanted.0 {
                return Err("SCM recovery served stale or incorrect ordinary-file bytes".into());
            }
        }
        Ok(())
    }
    fn restricted_denials(cache: &Path, run: &str) -> Result<Value> {
        let mut original = null_mut();
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_IMPERSONATE,
                &mut original,
            )
        } == 0
        {
            return Err(error("Open probe token"));
        }
        let original = Handle(original);
        let mut sid = [0u8; 68];
        let mut size = sid.len() as u32;
        if unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            )
        } == 0
        {
            return Err(error("Build disabled administrator SID"));
        }
        let disabled = SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        let mut token = null_mut();
        if unsafe {
            CreateRestrictedToken(
                original.0,
                DISABLE_MAX_PRIVILEGE,
                1,
                &disabled,
                0,
                null(),
                0,
                null(),
                &mut token,
            )
        } == 0
        {
            return Err(error("Create restricted probe token"));
        }
        let token = Handle(token);
        if unsafe { ImpersonateLoggedOnUser(token.0) } == 0 {
            return Err(error("Use restricted probe token"));
        }
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                if unsafe { RevertToSelf() } == 0 {
                    std::process::abort();
                }
            }
        }
        let revert = Revert;
        let paths = [
            Path::new(STATE).join(format!("denied-{run}.tmp")),
            cache.join(format!("denied-{run}.tmp")),
        ];
        let mut denied = Vec::new();
        let mut created = Vec::new();
        for path in &paths {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Err(e) => denied.push(e.raw_os_error() == Some(5)),
                Ok(file) => {
                    drop(file);
                    created.push(path.clone());
                    denied.push(false);
                }
            }
        }
        let manifest_denied = File::open(Path::new(STATE).join("manifest.json"))
            .err()
            .is_some_and(|e| e.raw_os_error() == Some(5));
        let admin_denied = OpenOptions::new()
            .read(true)
            .write(true)
            .open(r"\\.\pipe\SuperHDD.Admin.v5")
            .err()
            .is_some_and(|e| e.raw_os_error() == Some(5));
        drop(revert);
        for path in created {
            std::fs::remove_file(path)?;
        }
        if denied != vec![true, true] || !manifest_denied || !admin_denied {
            return Err("Restricted user token could access private SCM/cache state".into());
        }
        Ok(
            json!({"configCreateDenied":true,"cacheCreateDenied":true,"manifestReadDenied":true,"adminPipeDenied":true,"administratorSidDisabled":true,"maximumPrivilegesDisabled":true}),
        )
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        source_volume: String,
        container_root: PathBuf,
        budget_bytes: Option<u64>,
        acknowledged_disposable_source: bool,
        isolated_lab: LabIdentity,
    }
    pub fn run(tool: &Path, request_file: &Path, report: &Path) -> Result<()> {
        let request: Request = serde_json::from_reader(File::open(request_file)?)?;
        let lab = VerifiedLab::verify(request.isolated_lab.clone())?;
        lab.validate_root(&request.container_root)?;
        if request.source_volume != "S:"
            || !request.acknowledged_disposable_source
            || request.budget_bytes != Some(256 * 1024 * 1024)
            || request.container_root
                != Path::new(&format!("K:\\managed-{}", request.isolated_lab.run_id))
            || !tool.to_string_lossy().starts_with(&format!(
                "C:\\Program Files\\SuperHDDLab\\Managed-{}\\",
                request.isolated_lab.run_id
            ))
            || report.parent() != request_file.parent()
        {
            return Err("Use the exact fresh run-owned SCM fixture paths".into());
        }
        let controller = Controller::open(true)?;
        if controller.status()?.cache.attached {
            return Err("Require no existing kernel session".into());
        }
        let source = volume_identity("S:")?;
        if !matches!(source.bus_type, 1 | 10) {
            return Err("Not a disposable source".into());
        }
        let fixture = PathBuf::from(format!(
            "S:\\scm-fixture-{}.bin",
            request.isolated_lab.run_id
        ));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(0x20000000 | 0x10000000)
            .open(&fixture)?;
        let mut data = Box::new(Block([0; 65536]));
        for index in 0..4 {
            pattern(&mut data, index, 0);
            file.write_all(&data.0)?;
        }
        file.sync_all()?;
        let mut stage = "prepare";
        let mut svc = None;
        let mut owned_target = Value::Null;
        let test = (|| -> Result<Value> {
            cli(
                tool,
                "--prepare-direct-lab",
                request_file.to_str().ok_or("Invalid request path")?,
            )?;
            let _proof = ProtectedDir::open(Path::new(STATE))?;
            drop(_proof);
            stage = "install-start";
            svc = Some(Service::install(tool)?);
            let service = svc.as_ref().ok_or("Missing owned service")?;
            service.actions(true)?;
            service.start()?;
            let first = service.wait(SERVICE_RUNNING, 0)?;
            let initial = state(first.dwProcessId, "active")?;
            if initial["localSystem"] != true {
                return Err("SCM service was not running under LocalSystem".into());
            }
            owned_target = initial["target"].clone();
            let before = ipc::telemetry()?;
            for _ in 0..3 {
                reads(&mut file, 0)?;
            }
            let warm = ipc::telemetry()?;
            if warm.hits <= before.hits || warm.ssd_read_bytes <= before.ssd_read_bytes {
                return Err("SCM session did not serve actual SSD hits".into());
            }
            let acl = restricted_denials(&request.container_root, &request.isolated_lab.run_id)?;
            file.seek(SeekFrom::Start(0))?;
            pattern(&mut data, 0, 1);
            file.write_all(&data.0)?;
            file.sync_all()?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            stage = "normal-stop-reopen";
            service.stop()?;
            if controller.status()?.cache.attached || state(0, "stopped")?["target"] != Value::Null
            {
                return Err("Normal SCM stop did not clear kernel/durable ownership".into());
            }
            service.start()?;
            let normal = service.wait(SERVICE_RUNNING, first.dwProcessId)?;
            let reopened = state(normal.dwProcessId, "active")?;
            if reopened["recoveries"] != 0
                || reopened["target"]["containerInstance"] != initial["target"]["containerInstance"]
            {
                return Err("Clean SCM reopen replaced its container or claimed a crash".into());
            }
            reads(&mut file, 1)?;
            stage = "automatic-restart";
            let killed = service.kill(tool)?;
            // No StartService call here: only SCM failure recovery may restart it.
            let restarted = service.wait(SERVICE_RUNNING, killed)?;
            let recovered = state(restarted.dwProcessId, "active")?;
            owned_target = recovered["target"].clone();
            if recovered["recoveries"] != 1
                || recovered["recovery"]["kernelDisabled"] != true
                || recovered["target"]["containerInstance"]
                    != initial["target"]["containerInstance"]
            {
                return Err(format!(
                    "Automatic protected reconciliation not confirmed: {recovered}"
                )
                .into());
            }
            let imported = controller.status()?;
            for _ in 0..3 {
                reads(&mut file, 1)?;
            }
            let after = controller.status()?;
            if imported.imported_records == 0
                || after.revalidated_records == 0
                || after.cache.ssd_read_bytes <= imported.cache.ssd_read_bytes
                || after.cache.errors != 0
            {
                return Err("Recovered SCM data was not revalidated and served correctly".into());
            }
            stage = "owner-admin-controls";
            let pause = ipc::cache_action(ipc::CacheAction::Pause)?;
            if pause["phase"] != "paused" || !controller.status()?.cache.paused {
                return Err("Pause did not reach native session".into());
            }
            let paused_before = controller.status()?;
            reads(&mut file, 1)?;
            let paused_after = controller.status()?;
            if paused_before.cache.hits != paused_after.cache.hits {
                return Err("Paused reads used cache".into());
            }
            ipc::cache_action(ipc::CacheAction::Clear)?;
            ipc::cache_action(ipc::CacheAction::Resume)?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            let public_state = ipc::cache_status()?;
            if public_state["phase"] != "active" || controller.status()?.cache.paused {
                return Err("Resume/state contract failed".into());
            }
            Ok(
                json!({"passed":true,"checks":{"actualScmService":true,"protectedConfigAndCache":true,"restrictedUserDenied":true,
                "normalStopClearsOwnership":true,"sameContainerReopened":true,"actualScmAutomaticRestart":true,"noManualRecoveryCommand":true,
                "ownedKernelSessionReconciled":true,"ordinaryFileBytesAfterCrash":true,"persistentRecordsRevalidated":true,
                "adminPauseClearResume":true,"pausedReadsBypass":true,"publicStatusReadOnly":true},
                "initial":initial,"normalReopen":reopened,"recovered":recovered,"killedPid":killed,"restartedPid":restarted.dwProcessId,
                "acl":acl,"warmBefore":before,"warmAfter":warm,"imported":imported,"afterRecoveryReads":after,"publicState":public_state}),
            )
        })();
        let last_status = std::fs::read(Path::new(STATE).join("status.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        drop(svc.take());
        let cleanup = (|| -> Result<()> {
            let status = controller.status()?;
            if status.cache.attached {
                let own = Some(u64::from(status.source_disk_number))
                    == owned_target["sourceDisk"].as_u64()
                    && Some(u64::from(status.backing_disk_number))
                        == owned_target["cacheDisk"].as_u64();
                if !own {
                    return Err("Unknown kernel ownership during cleanup; preserved".into());
                }
                controller.disable()?;
            }
            cli(tool, "--remove-direct-lab", &request.isolated_lab.run_id)?;
            if Path::new(STATE).exists() {
                return Err("Protected service state remains".into());
            }
            if request.container_root.join("owner.json").exists() {
                let mut owned = OwnedVhd::reopen_in_lab_service(&request.container_root, lab)?;
                if let Some((disk, _)) = owned.inspect_attached_raw()? {
                    owned.detach_observed_raw(disk)?;
                }
                owned.remove()?;
            }
            if request.container_root.exists() {
                std::fs::remove_dir(&request.container_root)?;
            }
            drop(file);
            std::fs::remove_file(&fixture)?;
            Ok(())
        })();
        let mut value = match &test {
            Ok(value) => value.clone(),
            Err(error) => {
                json!({"passed":false,"stage":stage,"error":error.to_string(),"lastProtectedStatus":last_status})
            }
        };
        value["ownedFixturesRemoved"] = json!(cleanup.is_ok());
        if let Err(e) = &cleanup {
            value["passed"] = json!(false);
            value["cleanupError"] = json!(e.to_string());
        }
        value["scope"]=json!("Protected native SCM ownership and automatic process-crash recovery in the isolated signed-driver VM. No manual disable or StartService call occurs in the successful automatic-recovery window. The v5 source is an exact run-owned SAS data disk, using the VM-only driver policy.");
        File::create_new(report)?.write_all(&serde_json::to_vec_pretty(&value)?)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        test?;
        cleanup
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("direct_session_probe SERVICE.exe REQUEST.json NEW_REPORT.json\nProvisioned Super HDD VM only.");
        return Ok(());
    }
    #[cfg(windows)]
    if let [tool, request, report] = args.as_slice() {
        return proof::run(tool.as_ref(), request.as_ref(), report.as_ref());
    }
    Err("Windows VM and three explicit arguments required".into())
}
