//! Fixed physical enrollment. The request carries selections, never executable,
//! device or payload paths. Fresh native hardware checks precede allocation.
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use superhdd_platform::{
    cache_runtime::{Command, Operation},
    loopback_cache::Context,
    native_package,
};
use superhdd_storage_control::{
    loopback::{Config, ROOT},
    port_reservation::Reservation,
    recovery::{config_digest, Phase, RunJournal},
    windows::{
        self,
        protected::{self, ProtectedDir},
    },
};
type Result<T> = std::result::Result<T, String>;
struct Preparation {
    cache: Option<ProtectedDir>,
    payload: Option<File>,
    root: Option<ProtectedDir>,
    run_root: Option<ProtectedDir>,
    files: Vec<File>,
    reservation: Option<Reservation>,
    complete: bool,
}
impl Preparation {
    fn new_file(&mut self, name: &str) -> Result<File> {
        let file = self
            .root
            .as_ref()
            .ok_or("Preparation root absent")?
            .file(name, true)?;
        let duplicate = file.try_clone().map_err(|e| e.to_string());
        self.files.push(file);
        duplicate
    }
    fn save(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let mut file = self.new_file(name)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Some(reservation) = self.reservation.take() {
            let _ = reservation.remove();
        }
        for file in self.files.drain(..) {
            let _ = protected::remove_file(file);
        }
        if let Some(run) = self.run_root.take() {
            let path = run.path().to_path_buf();
            drop(run);
            let _ = std::fs::remove_dir(path);
        }
        if let Some(root) = self.root.take() {
            drop(root);
            let _ = std::fs::remove_dir(ROOT);
        }
        if let Some(payload) = self.payload.take() {
            let _ = protected::remove_file(payload);
        }
        if let Some(cache) = self.cache.take() {
            let path = cache.path().to_path_buf();
            drop(cache);
            let _ = std::fs::remove_dir(path);
        }
    }
}
pub fn configuration() -> Result<Option<(Config, Context)>> {
    if !Path::new(ROOT).join("manifest.json").exists() {
        return Ok(None);
    }
    let root = ProtectedDir::open(Path::new(ROOT))?;
    let read = |mut f: File| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        (&mut f)
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 32768 {
            return Err("Oversized native owner".into());
        }
        Ok(bytes)
    };
    let config: Config = serde_json::from_slice(&read(root.read_file("manifest.json")?)?)
        .map_err(|e| e.to_string())?;
    config.validate()?;
    if config.lab.is_some() {
        return Err("A disposable owner must be removed before physical enrollment".into());
    }
    let context: Context = serde_json::from_slice(&read(root.read_file("context.json")?)?)
        .map_err(|e| e.to_string())?;
    crate::loopback_host::validate_context(&context, &config)?;
    Ok(Some((config, context)))
}
fn prepare(context: Context) -> Result<Config> {
    if context.request.scope.is_some() {
        return Err("selectedScopeUnavailable".into());
    }
    if Path::new(ROOT).exists() {
        return Err("An existing native owner must be recovered before enrollment".into());
    }
    let mut guid = windows_sys::core::GUID::default();
    if unsafe { windows_sys::Win32::System::Com::CoCreateGuid(&mut guid) } < 0 {
        return Err("Cannot create runtime identity".into());
    }
    let run = format!(
        "{:08x}{:04x}{:04x}{}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let base = Path::new(&context.parent.volume_guid).join("SuperHDDCache");
    let _base = ProtectedDir::create(&base)?;
    let cache = ProtectedDir::create(&base.join(&run))?;
    let payload = cache.payload("payload.bin", true)?;
    let mut stage = Preparation {
        cache: Some(cache),
        payload: Some(payload),
        root: None,
        run_root: None,
        files: Vec::new(),
        reservation: None,
        complete: false,
    };
    let payload = stage.payload.as_ref().ok_or("Missing prepared payload")?;
    let config = Config {
        schema: 1,
        run,
        source: context.disk.clone(),
        source_volume: context.source.clone(),
        parent: context.parent.clone(),
        capacity_bytes: context.request.capacity_bytes,
        payload: windows::stable_file_identity(payload)?,
        source_fully_decrypted: true,
        lab: None,
    };
    config.validate()?;
    crate::loopback_host::validate_context(&context, &config)?;
    stage.root = Some(ProtectedDir::create(Path::new(ROOT))?);
    stage.run_root = Some(ProtectedDir::create(&Path::new(ROOT).join(&config.run))?);
    stage.save(
        "context.json",
        &serde_json::to_vec(&context).map_err(|e| e.to_string())?,
    )?;
    let bytes = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
    stage.save("manifest.json", &bytes)?;
    let mut journal = RunJournal::create(stage.new_file("lease.bin")?, config_digest(&bytes))?;
    journal.save(Phase::Prepared, Some(config.target()?))?;
    stage.reservation = Some(Reservation::create()?);
    stage.save(
        "port-owner.json",
        &serde_json::to_vec(
            stage
                .reservation
                .as_ref()
                .ok_or("Missing prepared reservation")?,
        )
        .map_err(|e| e.to_string())?,
    )?;
    // Persist ownership before the large allocation. Source stays online here.
    let payload = stage.payload.as_ref().ok_or("Missing prepared payload")?;
    payload
        .set_len(config.capacity_bytes)
        .and_then(|_| payload.sync_all())
        .map_err(|e| e.to_string())?;
    drop(journal);
    stage.complete = true;
    Ok(config)
}
pub fn run(command: Command) -> Result<()> {
    command.validate()?;
    let package = native_package::verify()?;
    if package.manifest.lab_only {
        return Err("The installed disposable package cannot enroll a physical volume".into());
    }
    if !std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .eq_ignore_ascii_case(&package.root.join("SuperHDDService.exe").to_string_lossy())
    {
        return Err("Physical operations require the protected installed helper".into());
    }
    let _operation = crate::runtime_ops::OperationGuard::acquire()?;
    match command.operation {
        Operation::Start => {
            let request = command.request.ok_or("Missing physical start request")?;
            if request.scope.is_some() {
                return Err("selectedScopeUnavailable".into());
            }
            let config = if let Some((config, context)) = configuration()? {
                if serde_json::to_value(&request).map_err(|e| e.to_string())?
                    != serde_json::to_value(&context.request).map_err(|e| e.to_string())?
                {
                    return Err(
                        "Remove the existing stopped allocation before changing its selections"
                            .into(),
                    );
                }
                config
            } else {
                prepare(Context::capture(request)?)?
            };
            crate::loopback_scm::start(&package.root, &config)
        }
        Operation::Pause => crate::loopback_scm::control(&package.root, 129, false),
        Operation::Resume => crate::loopback_scm::control(&package.root, 130, false),
        Operation::Clear => crate::loopback_scm::control(&package.root, 128, false),
        Operation::Stop => crate::loopback_scm::control(&package.root, 131, true),
        Operation::Remove => remove(&package.root),
    }
}
fn remove(package_root: &Path) -> Result<()> {
    let Some((config, context)) = configuration()? else {
        return Ok(());
    };
    if superhdd_storage_control::data_source::is_offline(&config.source)? {
        return Err("Stop the native cache and restore its source before removal".into());
    }
    context.revalidate()?;
    let root = ProtectedDir::open(Path::new(ROOT))?;
    let mut manifest = root.read_file("manifest.json")?;
    let mut bytes = Vec::new();
    (&mut manifest)
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Oversized physical owner".into());
    }
    let mut journal = RunJournal::open(root.file("lease.bin", false)?, config_digest(&bytes))?;
    if journal.record().target.as_ref() != Some(&config.target()?)
        && !(journal.record().phase == Phase::Stopped && journal.record().target.is_none())
    {
        return Err("Physical removal target differs from owned source".into());
    }
    let run_path = Path::new(ROOT).join(&config.run);
    let target = superhdd_storage_control::iscsi::managed_target_name(&config.run)?;
    if crate::loopback_scm::initiator_running()? {
        if superhdd_storage_control::iscsi::sessions()?
            .iter()
            .any(|s| s.target == target)
        {
            return Err("An owned exported session remains".into());
        }
        superhdd_storage_control::iscsi::remove_static_target(&target)?;
    } else if journal.record().phase != Phase::Prepared || run_path.join("ready.json").exists() {
        return Err("Initiator state must be available to prove the export is closed".into());
    }
    let cache = ProtectedDir::open(&config.cache_root()?)?;
    let payload = cache.payload("payload.bin", false)?;
    if windows::stable_file_identity(&payload)? != config.payload
        || payload.metadata().map_err(|e| e.to_string())?.len() > config.capacity_bytes
    {
        return Err("Physical payload identity changed; allocation preserved".into());
    }
    let mut port = root.file("port-owner.json", false)?;
    let mut data = Vec::new();
    (&mut port)
        .take(1025)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 1024 {
        return Err("Oversized endpoint owner".into());
    }
    let reservation: Reservation = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    reservation.validate()?;
    crate::loopback_scm::remove_service(package_root)?;
    journal.save(Phase::Stopped, None)?;
    drop(journal);
    crate::loopback_scm::restore_initiator(&root)?;
    reservation.remove()?;
    protected::remove_file(port)?;
    protected::remove_file(payload)?;
    let cache_path = config.cache_root()?;
    drop(cache);
    std::fs::remove_dir(cache_path).map_err(|e| e.to_string())?;
    let run_root = ProtectedDir::open(&run_path)?;
    for name in [
        "ready.json",
        "command",
        "ack",
        "stats.json",
        "stats.tmp",
        "peer-authorization.log",
        "service-state.json",
        "intent.json",
    ] {
        if run_path.join(name).exists() {
            protected::remove_file(run_root.file(name, false)?)?;
        }
    }
    drop(run_root);
    std::fs::remove_dir(run_path).map_err(|e| e.to_string())?;
    drop(manifest);
    for name in [
        "context.json",
        "initiator-owner.json",
        "supervisor.lock",
        "lease.bin",
        "manifest.json",
    ] {
        if Path::new(ROOT).join(name).exists() {
            protected::remove_file(root.file(name, false)?)?;
        }
    }
    drop(root);
    std::fs::remove_dir(ROOT).map_err(|e| e.to_string())
}
