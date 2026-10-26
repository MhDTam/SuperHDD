//! SCM supervisor for the protected native worker. Disposable and physical
//! entry points require matching package, configuration and source roles.
use serde::Deserialize;
use serde_json::json;
use std::{
    fs::File,
    io::{Read, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use superhdd_platform::native_package::{self, Verified};
use superhdd_storage_control::{
    data_source::{self, DiskIdentity},
    iscsi::{self, SessionId},
    loopback::{Config, Opened, ROOT},
    recovery::{config_digest, Phase, RunJournal},
    windows::{self, protected::ProtectedDir},
};
use windows_sys::Win32::{
    Foundation::{FILETIME, WAIT_OBJECT_0},
    System::Threading::*,
};
type Result<T> = std::result::Result<T, String>;
pub(crate) fn validate_context(
    context: &superhdd_platform::loopback_cache::Context,
    config: &Config,
) -> Result<()> {
    superhdd_platform::cache_runtime::Command {
        operation: superhdd_platform::cache_runtime::Operation::Start,
        request: Some(context.request.clone()),
    }
    .validate()?;
    let equal = |a: &superhdd_storage_control::VolumeIdentity,
                 b: &superhdd_storage_control::VolumeIdentity|
     -> Result<bool> {
        Ok(serde_json::to_value(a).map_err(|e| e.to_string())?
            == serde_json::to_value(b).map_err(|e| e.to_string())?)
    };
    if context.schema != 1
        || config.lab.is_some()
        || context.disk != config.source
        || !equal(&context.source, &config.source_volume)?
        || !equal(&context.parent, &config.parent)?
        || context.request.capacity_bytes != config.capacity_bytes
        || context
            .request
            .source_letter
            .eq_ignore_ascii_case(&context.request.ssd_letter)
    {
        return Err("Physical enrollment differs from protected worker ownership".into());
    }
    Ok(())
}
fn read<T: serde::de::DeserializeOwned>(mut file: File) -> Result<T> {
    let mut bytes = Vec::new();
    (&mut file)
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Oversized supervisor record".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn write(root: &ProtectedDir, name: &str, value: &serde_json::Value) -> Result<()> {
    let mut file = root.file(name, !root.path().join(name).exists())?;
    file.set_len(0).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ready {
    schema: u32,
    run_id: String,
    pid: u32,
    created: u64,
    source_bytes: u64,
}
enum Worker {
    Child(Child),
    Adopted(OwnedHandle),
}
impl Worker {
    fn handle(&self) -> windows_sys::Win32::Foundation::HANDLE {
        match self {
            Self::Child(p) => p.as_raw_handle(),
            Self::Adopted(p) => p.as_raw_handle(),
        }
    }
    fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle(), 0) == WAIT_OBJECT_0 }
    }
    fn adopt(ready: &Ready, path: &Path) -> Result<Self> {
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                ready.pid,
            )
        };
        if raw.is_null() {
            return Err("Cannot pin recorded worker process".into());
        }
        let pin = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(raw, &mut created, &mut exited, &mut kernel, &mut user) } == 0
            || ((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64)
                != ready.created
        {
            return Err("Recorded worker creation identity changed".into());
        }
        let mut text = [0u16; 1024];
        let mut count = text.len() as u32;
        if unsafe { QueryFullProcessImageNameW(raw, 0, text.as_mut_ptr(), &mut count) } == 0
            || !String::from_utf16_lossy(&text[..count as usize])
                .eq_ignore_ascii_case(&path.to_string_lossy())
        {
            return Err("Recorded worker executable differs from protected package".into());
        }
        let worker = Self::Adopted(pin);
        if worker.exited() {
            return Err("Recorded worker already exited".into());
        }
        Ok(worker)
    }
}
pub struct Host {
    package: Verified,
    _root: ProtectedDir,
    run_root: ProtectedDir,
    config: Config,
    journal: RunJournal,
    _manifest: File,
    _owner: File,
    worker: Option<Worker>,
    session: Option<SessionId>,
    projection: Option<DiskIdentity>,
    projection_volume: Option<superhdd_storage_control::VolumeIdentity>,
    counter: u64,
    draining: bool,
    last_mount_check: Instant,
    context: Option<superhdd_platform::loopback_cache::Context>,
    _context_pin: Option<File>,
    letter: char,
    public: std::sync::Arc<std::sync::Mutex<serde_json::Value>>,
    publication: std::sync::Mutex<(String, Option<String>)>,
    control_generation: u64,
    finished: bool,
}
impl Host {
    pub fn start(lab: bool) -> Result<Self> {
        if !crate::managed_host::is_system()? {
            return Err("Native SCM host requires LocalSystem".into());
        }
        let package = native_package::verify()?;
        let root = ProtectedDir::open(Path::new(ROOT))?;
        let mut manifest = root.read_file("manifest.json")?;
        let mut bytes = Vec::new();
        (&mut manifest)
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 32768 {
            return Err("Oversized protected configuration".into());
        }
        let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        config.validate()?;
        let reservation: superhdd_storage_control::port_reservation::Reservation =
            read(root.read_file("port-owner.json")?)?;
        reservation.validate()?;
        // No command line or SCM service name can turn this lab entry point into
        // a physical disk controller, even with a modified prepared manifest.
        if package.manifest.lab_only != lab || config.lab.is_some() != lab {
            return Err("SCM entry point, package and source roles differ".into());
        }
        let (context, context_pin, letter) = if lab {
            (None, None, 'S')
        } else {
            let pin = root.read_file("context.json")?;
            let context: superhdd_platform::loopback_cache::Context =
                read(pin.try_clone().map_err(|e| e.to_string())?)?;
            validate_context(&context, &config)?;
            let letter = context.request.source_letter.as_bytes()[0].to_ascii_uppercase() as char;
            (Some(context), Some(pin), letter)
        };
        let owner = root.file(
            "supervisor.lock",
            !Path::new(ROOT).join("supervisor.lock").exists(),
        )?;
        let journal = RunJournal::open(root.file("lease.bin", false)?, config_digest(&bytes))?;
        if !matches!(journal.record().phase, Phase::Prepared | Phase::Active)
            || journal.record().target.as_ref() != Some(&config.target()?)
        {
            return Err("Native SCM owner is not durably prepared".into());
        }
        let run_root = ProtectedDir::open(&Path::new(ROOT).join(&config.run))?;
        let mut host = Self {
            package,
            _root: root,
            run_root,
            config,
            journal,
            _manifest: manifest,
            _owner: owner,
            worker: None,
            session: None,
            projection: None,
            projection_volume: None,
            counter: 0,
            draining: false,
            last_mount_check: Instant::now(),
            context,
            _context_pin: context_pin,
            letter,
            public: std::sync::Arc::new(std::sync::Mutex::new(serde_json::Value::Null)),
            publication: std::sync::Mutex::new(("starting".into(), None)),
            control_generation: 0,
            finished: false,
        };
        host.state("starting", None)?;
        if let Err(error) = host.activate() {
            let recovery = host.rollback_before_mount();
            if recovery.is_ok() {
                let _ = host.state("startFailed", Some(&error));
                return Err(error);
            }
            // Preserve a live controller while exports or source restoration
            // still need management; never silently abandon an offline source.
            host.draining =
                host.worker.as_ref().is_none_or(Worker::exited) && host.session.is_none();
            host.state(
                "attention",
                Some(&format!("{error}; recovery={recovery:?}")),
            )?;
        }
        Ok(host)
    }
    fn state(&self, phase: &str, error: Option<&str>) -> Result<()> {
        write(
            &self.run_root,
            "service-state.json",
            &json!({"schema":1,"runId":self.config.run,"servicePid":std::process::id(),"phase":phase,"error":error,"controlGeneration":self.control_generation,"labOnly":self.config.lab.is_some(),"physicalDAccelerationActive":self.config.lab.is_none()&&self.letter=='D'&&phase=="active","sourceDisk":self.config.source.number,"sourceLetter":format!("{}:",self.letter),"projectedDisk":self.projection.as_ref().map(|p|p.number)}),
        )?;
        let stats: serde_json::Value = self
            .run_root
            .read_shared_file("stats.json")
            .and_then(read)
            .unwrap_or(serde_json::Value::Null);
        let paused = stats["paused"] == true;
        let attached = self.worker.as_ref().is_some_and(|w| !w.exited())
            && self.session.is_some()
            && self.projection.is_some();
        let runtime_phase = match phase {
            "active" if paused => "paused",
            "active" => "active",
            "stopped" => "stopped",
            "attention" | "startFailed" => "faulted",
            _ => "unknown",
        };
        let reason = error.map(|e| {
            if e.contains("Source busy") {
                "runtimeSourceBusy"
            } else {
                "runtimeOperationFailed"
            }
        });
        let s = &stats["statistics"];
        let number = |name: &str| s[name].as_u64().unwrap_or(0);
        let telemetry = json!({"protocolVersion":6,"experimentalAttachEnabled":false,"attached":attached,"paused":paused,"hits":number("hits"),"misses":number("misses"),"sourceReadBytes":number("sourceReadBytes"),"ssdReadBytes":number("ssdReadBytes"),"fillBytes":number("fillBytes"),"evictions":number("evictions"),"errors":number("backingErrors").saturating_add(number("checksumFailures")),"generation":stats["generation"].as_u64().unwrap_or(1),"capacityBytes":self.config.capacity_bytes,"sourceLength":self.config.source.bytes,"activeMutations":0,"pendingReads":0,"cacheDisabled":!attached,"readBypasses":number("sequentialBypasses"),"queueLimit":1,"maxReadBytes":1048576});
        let managed = self.draining || self.worker.as_ref().is_some_and(|w| !w.exited());
        let runtime = json!({"phase":runtime_phase,"canStart":false,"canControl":managed&&phase!="stopped"&&self.config.lab.is_none(),"canRemove":false,"reason":reason,"nativeVersion":6,"sourceVolume":format!("{}:",self.letter),"request":self.context.as_ref().map(|c|&c.request),"cacheUsedBytes":stats["occupancyBytes"].as_u64(),"labOnly":self.config.lab.is_some()});
        *self
            .public
            .lock()
            .map_err(|_| "Native public state unavailable")? =
            json!({"runtime":runtime,"telemetry":telemetry});
        *self
            .publication
            .lock()
            .map_err(|_| "Native publication state unavailable")? =
            (phase.into(), error.map(str::to_string));
        Ok(())
    }
    fn worker_path(&self) -> std::path::PathBuf {
        self.package.root.join("SuperHDDCacheWorker.exe")
    }
    fn ready(&self) -> Result<Ready> {
        let ready: Ready = read(self.run_root.read_shared_file("ready.json")?)?;
        if ready.schema != 1
            || ready.run_id != self.config.run
            || ready.source_bytes != self.config.source.bytes
            || ready.pid == 0
            || ready.created == 0
        {
            return Err("Worker ready marker differs from protected owner".into());
        }
        Ok(ready)
    }
    fn launch(&mut self) -> Result<()> {
        // This exclusive payload acquisition also proves that a prior worker no
        // longer owns the source/cache pair. The lease is released before spawn.
        drop(Opened::open(&self.config.run)?);
        let path = self.worker_path();
        let child = Command::new(&path)
            .arg(&self.config.run)
            .current_dir(&self.package.root)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
        let pid = child.id();
        self.worker = Some(Worker::Child(child));
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if self.worker.as_ref().is_none_or(Worker::exited) {
                return Err("Protected worker exited before readiness".into());
            }
            if let Ok(ready) = self.ready() {
                if ready.pid == pid {
                    Worker::adopt(&ready, &path)?;
                    return Ok(());
                }
            }
            if Instant::now() >= end {
                return Err("Protected worker readiness timed out".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn owned_session(&self) -> Result<Option<SessionId>> {
        let target = iscsi::managed_target_name(&self.config.run)?;
        let sessions: Vec<_> = iscsi::sessions()?
            .into_iter()
            .filter(|s| s.target == target)
            .collect();
        match sessions.as_slice() {
            [] => Ok(None),
            [s] if s.connections.len() <= 1
                && s.connections.iter().all(|c| {
                    c.target_address == "127.0.0.1"
                        && c.target_port == superhdd_storage_control::port_reservation::PORT
                }) =>
            {
                Ok(Some(s.id))
            }
            _ => Err("Ambiguous owned session or endpoint".into()),
        }
    }
    fn activate(&mut self) -> Result<()> {
        let offline = data_source::is_offline(&self.config.source)?;
        if let Ok(ready) = self.ready() {
            if let Ok(worker) = Worker::adopt(&ready, &self.worker_path()) {
                if !offline {
                    return Err("A recorded live worker exists while its source is online".into());
                }
                self.worker = Some(worker);
            }
        }
        if self.worker.is_none() {
            if !offline {
                if self.owned_session()?.is_some() {
                    return Err("An existing projection prevents a new source handoff".into());
                }
                // Revalidate the exact attached file-backed dependency before
                // locking. A physical disk cannot pass the fixed lab role.
                if let Some(lab) = self.config.lab.as_ref() {
                    let deps = windows::disk_dependencies(&self.config.target()?.source_device)?;
                    let [dependency] = deps.as_slice() else {
                        return Err("Ambiguous disposable dependency".into());
                    };
                    let file = std::fs::OpenOptions::new()
                        .access_mode(windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES)
                        .share_mode(3)
                        .custom_flags(
                            windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
                        )
                        .open(
                            Path::new(&lab.host_volume)
                                .join(format!("SuperHDD-handoff-{}.vhdx", self.config.run)),
                        )
                        .map_err(|e| e.to_string())?;
                    if windows::stable_file_identity(&file)? != lab.file
                        || !dependency.microsoft_provider
                        || dependency.device_id != 3
                        || dependency.ancestor_level != 0
                        || dependency.provider_flags != 0
                        || !dependency
                            .host_volume
                            .trim_end_matches('\\')
                            .eq_ignore_ascii_case(lab.host_volume.trim_end_matches('\\'))
                        || !dependency.relative_path.eq_ignore_ascii_case(&format!(
                            "\\SuperHDD-handoff-{}.vhdx",
                            self.config.run
                        ))
                    {
                        return Err(
                            "Disposable source dependency changed before SCM handoff".into()
                        );
                    }
                } else {
                    self.context
                        .as_ref()
                        .ok_or("Missing physical enrollment context")?
                        .revalidate()?;
                }
                data_source::lock_and_offline(&self.config.source_volume, &self.config.source)?;
                data_source::release_letter(
                    &self.config.source_volume,
                    &self.config.source,
                    self.letter,
                )?;
            }
            self.launch()?;
        }
        self.session = self.owned_session()?;
        if self.session.is_none() {
            let target = iscsi::managed_target_name(&self.config.run)?;
            iscsi::remove_static_target(&target)?;
            self.session = Some(iscsi::connect(&target)?);
        }
        self.mount_projection()?;
        self.journal
            .save(Phase::Active, Some(self.config.target()?))?;
        self.state("active", None)
    }
    fn mount_projection(&mut self) -> Result<()> {
        let session = self.session.ok_or("No owned session")?;
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let attempt =
                (|| -> Result<(DiskIdentity, superhdd_storage_control::VolumeIdentity)> {
                    let devices = iscsi::devices(session)?;
                    let [device] = devices.as_slice() else {
                        return Err("One projected LUN required".into());
                    };
                    if device.device_type != 7 {
                        return Err("Projected LUN is not a disk".into());
                    }
                    let projection = data_source::inspect_projection(
                        &self.config.source,
                        device.disk_number,
                        &self.config.run,
                    )?;
                    if data_source::is_offline(&projection)? {
                        data_source::restore_online(&projection)?;
                    }
                    let volume = bind_ready(&projection, self.letter)?;
                    Ok((projection, volume))
                })();
            match attempt {
                Ok((projection, volume)) => {
                    self.projection = Some(projection);
                    self.projection_volume = Some(volume);
                    return Ok(());
                }
                Err(e) if Instant::now() >= end => return Err(e),
                Err(_) => thread::sleep(Duration::from_millis(200)),
            }
        }
    }
    pub fn action(&mut self, operation: u32) -> Result<()> {
        if !matches!(operation, 0..=3 | 5) {
            return Err("Unknown protected worker operation".into());
        }
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or("Command counter exhausted")?;
        let token = format!("{:016x}{:016x}", std::process::id(), self.counter);
        let mut command = self
            .run_root
            .file("command", !self.run_root.path().join("command").exists())?;
        command.set_len(0).map_err(|e| e.to_string())?;
        write!(command, "{token} {operation}")
            .and_then(|_| command.sync_all())
            .map_err(|e| e.to_string())?;
        drop(command);
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(mut ack) = self.run_root.read_shared_file("ack") {
                let mut text = String::new();
                (&mut ack)
                    .take(129)
                    .read_to_string(&mut text)
                    .map_err(|e| e.to_string())?;
                if text.trim() == format!("{token} 0") {
                    if matches!(operation, 2 | 3) {
                        write(
                            &self.run_root,
                            "intent.json",
                            &json!({"schema":1,"runId":self.config.run,"paused":operation==2}),
                        )?;
                    }
                    return Ok(());
                } else if text.starts_with(&token) {
                    return Err("Worker refused the requested operation".into());
                }
            }
            if Instant::now() >= end {
                return Err("Worker acknowledgement timed out".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn poll(&mut self) -> Result<()> {
        if self.draining {
            self.stop()?;
            self.finished = true;
            return Ok(());
        }
        if self.worker.as_ref().is_none_or(Worker::exited) {
            self.state("recoveringWorker", None)?;
            self.worker = None;
            self.launch()?;
            self.session = self.owned_session()?;
            if self.session.is_none() {
                let target = iscsi::managed_target_name(&self.config.run)?;
                iscsi::remove_static_target(&target)?;
                self.session = Some(iscsi::connect(&target)?);
            }
            self.mount_projection()?;
            self.state("active", None)?;
        }
        // Arrival/reconnect can remove an early mount point after the disk has
        // already reported its identity. Keep checking the actual NTFS root,
        // rather than equating an assigned letter with a mounted filesystem.
        if self.last_mount_check.elapsed() >= Duration::from_secs(1) {
            self.last_mount_check = Instant::now();
            if self.session.is_none() {
                let target = iscsi::managed_target_name(&self.config.run)?;
                self.session = self.owned_session()?;
                if self.session.is_none() {
                    iscsi::remove_static_target(&target)?;
                    self.session = Some(iscsi::connect(&target)?);
                }
            }
            if std::fs::metadata(format!("{}:\\", self.letter)).is_err() {
                self.mount_projection()?;
                self.state("active", None)?;
            }
            self.action(0)?;
            let (phase, error) = self
                .publication
                .lock()
                .map_err(|_| "Native publication state unavailable")?
                .clone();
            self.state(&phase, error.as_deref())?;
        }
        Ok(())
    }
    pub fn error(&self, error: &str) {
        let _ = self.state("attention", Some(error));
    }
    pub fn stop(&mut self) -> Result<()> {
        // A busy projected NTFS refuses locking; keep the worker and original
        // offline source alive and report Running to SCM so stop can be retried.
        if let Some(session) = self.owned_session()? {
            let devices = iscsi::devices(session)?;
            let [device] = devices.as_slice() else {
                return Err("Cannot drain ambiguous projection".into());
            };
            let projection = data_source::inspect_projection(
                &self.config.source,
                device.disk_number,
                &self.config.run,
            )?;
            let volume = self
                .projection_volume
                .clone()
                .ok_or("Missing recorded live projection volume")?;
            if !data_source::is_offline(&projection)? {
                data_source::lock_and_offline(&volume, &projection)?;
            }
            self.draining = true;
            data_source::release_letter(&volume, &projection, self.letter)?;
            iscsi::disconnect(&iscsi::managed_target_name(&self.config.run)?, session)?;
        }
        self.session = None;
        self.draining = true;
        iscsi::remove_static_target(&iscsi::managed_target_name(&self.config.run)?)?;
        if self.worker.as_ref().is_some_and(|w| !w.exited()) {
            self.action(5)?;
            let end = Instant::now() + Duration::from_secs(15);
            while self.worker.as_ref().is_some_and(|w| !w.exited()) {
                if Instant::now() >= end {
                    return Err("Drained worker did not exit; source remains offline".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        self.worker = None;
        // Reacquire the exact exclusive SSD handle before returning the original
        // filesystem. This also rejects an unrecorded worker retaining its lease.
        let cache = ProtectedDir::open(&self.config.cache_root()?)?;
        let payload = cache.payload("payload.bin", false)?;
        if windows::stable_file_identity(&payload)? != self.config.payload {
            return Err("Original payload identity changed before source restoration".into());
        }
        if data_source::is_offline(&self.config.source)? {
            data_source::restore_online(&self.config.source)?;
        }
        wait_source_mount(&self.config.source, self.letter)?;
        self.projection = None;
        self.journal.save(Phase::Stopped, None)?;
        self.state("stopped", None)
    }
    fn rollback_before_mount(&mut self) -> Result<()> {
        if self.owned_session()?.is_some() || self.worker.as_ref().is_some_and(|w| !w.exited()) {
            return Err("An owned export remains; protected state preserved for restart".into());
        }
        if data_source::is_offline(&self.config.source)? {
            drop(Opened::open(&self.config.run)?);
            data_source::restore_online(&self.config.source)?;
            wait_source_mount(&self.config.source, self.letter)?;
        }
        Ok(())
    }
}

fn bind_ready(
    expected: &DiskIdentity,
    letter: char,
) -> Result<superhdd_storage_control::VolumeIdentity> {
    let volume = data_source::ensure_letter(expected, letter)?;
    if windows::volume_filesystem(&format!("{letter}:"))? != "NTFS"
        || !std::fs::metadata(format!("{letter}:\\"))
            .map_err(|e| e.to_string())?
            .is_dir()
    {
        return Err("Owned NTFS root has not mounted".into());
    }
    let current = windows::volume_identity(&format!("{letter}:"))?;
    if data_source::inspect_volume(&current)? != *expected {
        return Err("Owned root identity changed during mount arrival".into());
    }
    Ok(volume)
}
fn wait_source_mount(expected: &DiskIdentity, letter: char) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        match bind_ready(expected, letter) {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() >= end => return Err(e),
            Err(_) => thread::sleep(Duration::from_millis(200)),
        }
    }
}

windows_service::define_windows_service!(service_entry, service_main);
static LAB_MODE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
pub fn start_scm(lab: bool) -> std::result::Result<(), windows_service::Error> {
    let _ = LAB_MODE.set(lab);
    windows_service::service_dispatcher::start(
        if lab {
            native_package::LAB_SERVICE
        } else {
            native_package::SERVICE
        },
        service_entry,
    )
}
fn service_main(_args: Vec<std::ffi::OsString>) {
    let _ = run_scm();
}
fn run_scm() -> Result<()> {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };
    use windows_service::{
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
    };
    let pending = Arc::new(AtomicU32::new(0));
    let queued = pending.clone();
    let lab = *LAB_MODE.get().ok_or("Missing SCM role")?;
    let status = service_control_handler::register(
        if lab {
            native_package::LAB_SERVICE
        } else {
            native_package::SERVICE
        },
        move |event| {
            let operation = match event {
                ServiceControl::Interrogate => return ServiceControlHandlerResult::NoError,
                ServiceControl::UserEvent(code) => match code.to_raw() {
                    128 => 1,
                    129 => 2,
                    130 => 3,
                    131 => 5,
                    132 => 6,
                    _ => return ServiceControlHandlerResult::NotImplemented,
                },
                _ => return ServiceControlHandlerResult::NotImplemented,
            };
            if queued
                .compare_exchange(0, operation, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                ServiceControlHandlerResult::NoError
            } else {
                ServiceControlHandlerResult::Other(1061)
            }
        },
    )
    .map_err(|e| e.to_string())?;
    let set = |state, accepted, exit, wait| {
        status
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: accepted,
                exit_code: exit,
                checkpoint: if state == ServiceState::StartPending {
                    1
                } else {
                    0
                },
                wait_hint: wait,
                process_id: None,
            })
            .map_err(|e| e.to_string())
    };
    set(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        ServiceExitCode::Win32(0),
        Duration::from_secs(120),
    )?;
    let mut host = match Host::start(lab) {
        Ok(host) => host,
        Err(error) => {
            set(
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                ServiceExitCode::ServiceSpecific(1),
                Duration::ZERO,
            )?;
            return Err(error);
        }
    };
    let public_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let public_flag = public_stop.clone();
    let public_state = host.public.clone();
    let public_thread = thread::spawn(move || {
        superhdd_platform::ipc::serve_native(lab, public_flag, |request| {
            let key = match request.get("command").and_then(serde_json::Value::as_str) {
                Some("cache_status") => "runtime",
                Some("telemetry") => "telemetry",
                _ => return None,
            };
            Some(match public_state.lock() {
                Ok(state) => json!({"ok":true,"result":state[key]}),
                Err(_) => json!({"ok":false,"error":"Native state unavailable"}),
            })
        })
    });
    set(
        ServiceState::Running,
        // SCM stops forwarding controls after a standard STOP, even if NTFS
        // refuses its lock. Use retryable custom 131 for a safe drain instead.
        // https://learn.microsoft.com/en-us/windows/win32/services/service-control-handler-function
        ServiceControlAccept::empty(),
        ServiceExitCode::Win32(0),
        Duration::ZERO,
    )?;
    loop {
        let operation = pending.swap(0, Ordering::SeqCst);
        if matches!(operation, 1..=3 | 5) {
            host.control_generation = host
                .control_generation
                .checked_add(1)
                .ok_or("Control generation exhausted")?;
            host.state("controlling", None)?;
        }
        if operation == 5 {
            match host.stop() {
                Ok(()) => break,
                Err(error) => {
                    host.error(&error);
                    set(
                        ServiceState::Running,
                        ServiceControlAccept::empty(),
                        ServiceExitCode::Win32(0),
                        Duration::ZERO,
                    )?;
                }
            }
        } else if operation != 0 {
            if let Err(error) = host.action(if operation == 6 { 0 } else { operation }) {
                host.error(&error);
            } else if operation != 6 {
                let _ = host.state("active", None);
            }
        }
        if let Err(error) = host.poll() {
            host.error(&error);
            thread::sleep(Duration::from_secs(1));
        }
        if host.finished {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    public_stop.store(true, Ordering::SeqCst);
    let _ = public_thread.join();
    set(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        ServiceExitCode::Win32(0),
        Duration::ZERO,
    )
}
