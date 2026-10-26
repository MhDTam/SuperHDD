//! SCM-owned session, currently enabled only by the explicitly prepared VM mode.
//! The normal read-only service never loads or creates this configuration.
use crate::lab_control::Request;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use superhdd_platform::{
    managed_cache::{self, ManagedCache},
    raw_lab::Controller,
};
use superhdd_storage_control::{
    recovery::{config_digest, Phase, RunJournal},
    session::Target,
    windows::{
        disk_dependencies,
        protected::{self, ProtectedDir},
        volume_filesystem, volume_identity, VerifiedLab,
    },
    ContainerBudget,
};
pub const ROOT: &str = r"C:\ProgramData\SuperHDDManagedLab";
pub(crate) fn is_system() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError},
        Security::{
            GetTokenInformation, IsWellKnownSid, TokenUser, WinLocalSystemSid, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(
            std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32).to_string(),
        );
    }
    let mut buffer = [0u64; 256];
    let mut used = 0;
    let success = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut used,
        )
    } != 0;
    let value = success
        && used as usize >= size_of::<TOKEN_USER>()
        && used as usize <= buffer.len() * 8
        && unsafe {
            IsWellKnownSid(
                (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                WinLocalSystemSid,
            )
        } != 0;
    unsafe {
        CloseHandle(token);
    }
    Ok(value)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: u32,
    request: Request,
}
fn read(file: &mut File) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8192 {
        return Err("Protected service manifest exceeds its limit.".into());
    }
    Ok(bytes)
}
fn verified(request: &Request) -> Result<VerifiedLab, String> {
    if !request.acknowledged_disposable_source || !request.container_root.is_absolute() {
        return Err("Missing service source ownership acknowledgement.".into());
    }
    let identity = request
        .isolated_lab
        .clone()
        .ok_or("Managed SCM mode requires the provisioned VM context")?;
    let lab = VerifiedLab::verify(identity)?;
    lab.validate_root(&request.container_root)?;
    let budget = ContainerBudget::new(
        request
            .budget_bytes
            .ok_or("Managed service requires a fixed recorded budget")?,
    )?;
    if budget.total_bytes > 512 * 1024 * 1024 {
        return Err("Lab service budget exceeds 512 MiB.".into());
    }
    Ok(lab)
}
pub fn prepare(file: &Path) -> Result<(), String> {
    let mut request = crate::lab_control::read_request(file)?;
    // No protected directory, disk or config is touched before this VM guard.
    let lab = verified(&request)?;
    let driver = Controller::open()?;
    if driver.status()?.cache.attached {
        return Err("Stop the existing lab session before preparing the SCM owner.".into());
    }
    let source = volume_identity(&request.source_volume)?;
    if source.bus_type != 15
        || source.removable
        || volume_filesystem(&request.source_volume)? != "NTFS"
    {
        return Err("Require the run-owned file-backed NTFS source.".into());
    }
    let dependencies = disk_dependencies(&format!("\\\\.\\PhysicalDrive{}", source.disk_number))?;
    let [dependency] = dependencies.as_slice() else {
        return Err("Unproven service source ancestry".into());
    };
    let parent = volume_identity(&format!(
        "{}\\",
        dependency.host_volume.trim_end_matches('\\')
    ))?;
    lab.validate_source_backing(&parent, &dependencies)?;
    lab.validate_parent(&volume_identity(&lab.identity().cache_parent_volume)?)?;
    request.source_volume = source.volume_guid;
    let _cache = ProtectedDir::create(&request.container_root)?;
    let root = ProtectedDir::create(Path::new(ROOT))?;
    let mut manifest = root.file("manifest.json", true)?;
    let bytes =
        serde_json::to_vec_pretty(&Manifest { schema: 1, request }).map_err(|e| e.to_string())?;
    manifest
        .write_all(&bytes)
        .and_then(|_| manifest.sync_all())
        .map_err(|e| e.to_string())?;
    let journal = RunJournal::create(root.file("lease.bin", true)?, config_digest(&bytes))?;
    let mut status = root.file("status.json", true)?;
    status
        .write_all(
            &serde_json::to_vec(
                &json!({"phase":"prepared","sequence":journal.sequence(),"recoveries":0}),
            )
            .map_err(|e| e.to_string())?,
        )
        .and_then(|_| status.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
struct Store {
    _root: ProtectedDir,
    _manifest: File,
    _cache: ProtectedDir,
    request: Request,
    journal: RunJournal,
    status: File,
}
impl Store {
    fn open() -> Result<Self, String> {
        let root = ProtectedDir::open(Path::new(ROOT))?;
        let mut manifest = root.file("manifest.json", false)?;
        let bytes = read(&mut manifest)?;
        let data: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if data.schema != 1 {
            return Err("Unknown protected service manifest schema.".into());
        }
        verified(&data.request)?;
        let journal = RunJournal::open(root.file("lease.bin", false)?, config_digest(&bytes))?;
        let cache = ProtectedDir::open(&data.request.container_root)?;
        let status = root.file("status.json", false)?;
        Ok(Self {
            _root: root,
            _manifest: manifest,
            _cache: cache,
            request: data.request,
            journal,
            status,
        })
    }
    fn status(
        &mut self,
        phase: &str,
        target: Option<&Target>,
        recovery: Option<&managed_cache::RecoveryOutcome>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let value = json!({"phase":phase,"pid":std::process::id(),"localSystem":is_system()?,"sequence":self.journal.sequence(),"recoveries":self.journal.record().recoveries,
            "target":target,"recovery":recovery,"error":error,"runId":self.request.isolated_lab.as_ref().map(|i|&i.run_id)});
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        self.status
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.status.write_all(&bytes))
            .and_then(|_| self.status.set_len(bytes.len() as u64))
            .and_then(|_| self.status.sync_all())
            .map_err(|e| e.to_string())
    }
}
pub struct ManagedHost {
    session: ManagedCache,
    store: Store,
}
impl ManagedHost {
    pub fn start() -> Result<Self, String> {
        if !is_system()? {
            return Err("Managed SCM ownership requires the LocalSystem service account.".into());
        }
        let mut store = Store::open()?;
        let result =
            (|| -> Result<(ManagedCache, Option<managed_cache::RecoveryOutcome>), String> {
                let recovery = if let Some(target) = store.journal.record().target.clone() {
                    let outcome = managed_cache::recover_isolated_lab(
                        &store.request.source_volume,
                        &store.request.container_root,
                        store
                            .request
                            .isolated_lab
                            .clone()
                            .ok_or("Missing VM context")?,
                        &target,
                    )?;
                    store.journal.reconciled()?;
                    Some(outcome)
                } else {
                    None
                };
                let budget = if store.request.container_root.join("owner.json").exists() {
                    None
                } else {
                    Some(ContainerBudget::new(
                        store.request.budget_bytes.ok_or("Missing service budget")?,
                    )?)
                };
                let mut session = managed_cache::open_isolated_service(
                    &store.request.source_volume,
                    &store.request.container_root,
                    budget,
                    store
                        .request
                        .isolated_lab
                        .clone()
                        .ok_or("Missing VM context")?,
                )?;
                session.start_with_checkpoint(|target| {
                    store.journal.save(Phase::Prepared, Some(target.clone()))
                })?;
                store
                    .journal
                    .save(Phase::Active, session.target().cloned())?;
                Ok((session, recovery))
            })();
        match result {
            Ok((session, recovery)) => {
                store.status("active", session.target(), recovery.as_ref(), None)?;
                Ok(Self { session, store })
            }
            Err(error) => {
                let _ = store.status("faulted", None, None, Some(&error));
                Err(error)
            }
        }
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.store
            .status("stopping", self.session.target(), None, None)?;
        self.session.stop()?;
        self.store.journal.save(Phase::Stopped, None)?;
        self.store.status("stopped", None, None, None)
    }
}
pub fn remove(run_id: &str) -> Result<(), String> {
    let store = Store::open()?;
    if store
        .request
        .isolated_lab
        .as_ref()
        .map(|i| i.run_id.as_str())
        != Some(run_id)
    {
        return Err("Protected service configuration belongs to another run; preserved.".into());
    }
    if Controller::open()?.status()?.cache.attached {
        return Err("Cannot remove configuration while the kernel holds a session.".into());
    }
    drop(store);
    let root = ProtectedDir::open(Path::new(ROOT))?;
    // Exact files opened with exclusive write ownership and verified ACL/type.
    for name in ["manifest.json", "lease.bin", "status.json"] {
        protected::remove_file(root.file(name, false)?)?;
    }
    drop(root);
    std::fs::remove_dir(ROOT).map_err(|e| e.to_string())
}
