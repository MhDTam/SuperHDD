//! LocalSystem v5 owner. Public IPC reads state; a separate BA/SY pipe controls it.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use superhdd_platform::{
    direct_cache::{self, Context, DirectCache},
    direct_driver::Controller,
    ipc::CacheAction,
};
use superhdd_storage_control::{
    recovery::{config_digest, Phase, RunJournal},
    windows::protected::{self, ProtectedDir},
    ContainerBudget,
};
pub const ROOT: &str = r"C:\ProgramData\SuperHDDDirect";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema: u32,
    pub context: Context,
    pub container_root: PathBuf,
    pub budget_bytes: u64,
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || !self.container_root.is_absolute() {
            return Err("Unknown direct service configuration".into());
        }
        ContainerBudget::new(self.budget_bytes)?;
        if let Some(id) = &self.context.isolated_lab {
            let lab = superhdd_storage_control::windows::VerifiedLab::verify(id.clone())?;
            lab.validate_root(&self.container_root)?;
            if self.budget_bytes > 512 * 1024 * 1024 {
                return Err("Lab budget exceeds 512 MiB".into());
            }
        }
        Ok(())
    }
}
pub fn configuration() -> Result<Option<Config>, String> {
    if !Path::new(ROOT).exists() {
        return Ok(None);
    }
    let _root = ProtectedDir::open(Path::new(ROOT))?;
    let file = File::open(Path::new(ROOT).join("manifest.json")).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8192 {
        return Err("Oversized protected configuration".into());
    }
    let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    config.validate()?;
    Ok(Some(config))
}
pub fn prepare_lab(path: &Path) -> Result<(), String> {
    let r = crate::lab_control::read_request(path)?;
    let lab = r
        .isolated_lab
        .ok_or("Use the isolated VM context for this developer command")?;
    let context = Context::capture(&r.source_volume, &r.container_root, Some(lab))?;
    prepare(Config {
        schema: 1,
        context,
        container_root: r.container_root,
        budget_bytes: r.budget_bytes.ok_or("Missing fixed budget")?,
    })
}
pub fn prepare(config: Config) -> Result<(), String> {
    config.validate()?;
    if Controller::open(config.context.isolated_lab.is_some())?
        .status()?
        .cache
        .attached
    {
        return Err("Stop the existing session before setup".into());
    }
    let _cache = ProtectedDir::create(&config.container_root)?;
    let root = ProtectedDir::create(Path::new(ROOT))?;
    let mut file = root.file("manifest.json", true)?;
    let bytes = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    RunJournal::create(root.file("lease.bin", true)?, config_digest(&bytes))?;
    let mut status = root.file("status.json", true)?;
    status
        .write_all(b"{\"phase\":\"prepared\"}")
        .and_then(|_| status.sync_all())
        .map_err(|e| e.to_string())
}
struct Store {
    _root: ProtectedDir,
    _manifest: File,
    _cache: ProtectedDir,
    config: Config,
    journal: RunJournal,
    status: File,
    last: Value,
}
impl Store {
    fn open() -> Result<Self, String> {
        let root = ProtectedDir::open(Path::new(ROOT))?;
        let mut file = root.file("manifest.json", false)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8192 {
            return Err("Oversized protected configuration".into());
        }
        let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        config.validate()?;
        let journal = RunJournal::open(root.file("lease.bin", false)?, config_digest(&bytes))?;
        let cache = ProtectedDir::open(&config.container_root)?;
        let status = root.file("status.json", false)?;
        Ok(Self {
            _root: root,
            _manifest: file,
            _cache: cache,
            config,
            journal,
            status,
            last: Value::Null,
        })
    }
    fn save(
        &mut self,
        phase: &str,
        target: Option<&superhdd_storage_control::session::Target>,
        recovery: Option<Value>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let value = json!({"phase":phase,"pid":std::process::id(),"localSystem":crate::managed_host::is_system()?,
            "sequence":self.journal.sequence(),"recoveries":self.journal.record().recoveries,"target":target,
            "recovery":recovery,"error":error,"vmTestBuild":self.config.context.isolated_lab.is_some()});
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        self.status
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.status.write_all(&bytes))
            .and_then(|_| self.status.set_len(bytes.len() as u64))
            .and_then(|_| self.status.sync_all())
            .map_err(|e| e.to_string())?;
        self.last = value;
        Ok(())
    }
}
pub struct DirectHost {
    session: DirectCache,
    store: Store,
}
impl DirectHost {
    pub fn start() -> Result<Self, String> {
        if !crate::managed_host::is_system()? {
            return Err("Direct service requires LocalSystem".into());
        }
        let mut store = Store::open()?;
        let result = (|| -> Result<(DirectCache, Option<Value>), String> {
            let recovery = if let Some(target) = store.journal.record().target.clone() {
                let result = direct_cache::recover(
                    &store.config.context,
                    &store.config.container_root,
                    &target,
                )?;
                store.journal.reconciled()?;
                Some(serde_json::to_value(result).map_err(|e| e.to_string())?)
            } else {
                None
            };
            let budget = if store.config.container_root.join("owner.json").exists() {
                None
            } else {
                Some(ContainerBudget::new(store.config.budget_bytes)?)
            };
            let mut session = direct_cache::open(
                store.config.context.clone(),
                &store.config.container_root,
                budget,
            )?;
            session
                .start_with_checkpoint(|t| store.journal.save(Phase::Prepared, Some(t.clone())))?;
            store
                .journal
                .save(Phase::Active, session.target().cloned())?;
            Ok((session, recovery))
        })();
        match result {
            Ok((session, recovery)) => {
                store.save("active", session.target(), recovery, None)?;
                Ok(Self { session, store })
            }
            Err(error) => {
                let _ = store.save("faulted", None, None, Some(&error));
                Err(error)
            }
        }
    }
    pub fn state(&self) -> Value {
        self.store.last.clone()
    }
    pub fn action(&mut self, action: CacheAction) -> Result<Value, String> {
        let result = match action {
            CacheAction::Pause => self.session.pause(true),
            CacheAction::Resume => self.session.pause(false),
            CacheAction::Clear => self.session.clear(),
        };
        let phase = match self.session.phase() {
            superhdd_storage_control::session::Phase::Active => "active",
            superhdd_storage_control::session::Phase::Paused => "paused",
            superhdd_storage_control::session::Phase::Stopped => "stopped",
            _ => "faulted",
        };
        self.store.save(
            phase,
            self.session.target(),
            None,
            result.as_ref().err().map(String::as_str),
        )?;
        result?;
        Ok(self.state())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.store
            .save("stopping", self.session.target(), None, None)?;
        self.session.stop()?;
        self.store.journal.save(Phase::Stopped, None)?;
        self.store.save("stopped", None, None, None)
    }
}
pub fn remove_lab(run_id: &str) -> Result<(), String> {
    let store = Store::open()?;
    if store
        .config
        .context
        .isolated_lab
        .as_ref()
        .map(|i| i.run_id.as_str())
        != Some(run_id)
    {
        return Err("Configuration belongs to another owner".into());
    }
    if Controller::open(true)?.status()?.cache.attached {
        return Err("Kernel session still attached".into());
    }
    drop(store);
    let root = ProtectedDir::open(Path::new(ROOT))?;
    for name in ["manifest.json", "lease.bin", "status.json"] {
        protected::remove_file(root.file(name, false)?)?;
    }
    drop(root);
    std::fs::remove_dir(ROOT).map_err(|e| e.to_string())
}
pub fn remove_production() -> Result<(), String> {
    let store = Store::open()?;
    if store.config.context.isolated_lab.is_some() {
        return Err("Production removal cannot adopt a lab owner".into());
    }
    if Controller::open(false)?.status()?.cache.attached {
        return Err("Kernel still holds a session".into());
    }
    let cache_root = store.config.container_root.clone();
    if cache_root.join("owner.json").exists() {
        let mut owned = superhdd_storage_control::windows::OwnedVhd::reopen_service(&cache_root)?;
        if let Some((disk, _)) = owned.inspect_attached_raw()? {
            owned.detach_observed_raw(disk)?;
        }
        owned.remove()?;
    }
    std::fs::remove_dir(&cache_root).map_err(|e| e.to_string())?;
    drop(store);
    let root = ProtectedDir::open(Path::new(ROOT))?;
    for name in ["manifest.json", "lease.bin", "status.json"] {
        protected::remove_file(root.file(name, false)?)?;
    }
    drop(root);
    std::fs::remove_dir(ROOT).map_err(|e| e.to_string())
}
