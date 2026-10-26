//! Stages only a fresh owned 128 MiB VHDX fixture. This developer command cannot
//! enroll a physical source; the real-device preflight remains read-only.
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use superhdd_storage_control::{
    data_source, iscsi,
    loopback::{Config, LabSource, ROOT},
    recovery::{config_digest, Phase, RunJournal},
    windows::{
        self,
        protected::{self, ProtectedDir},
    },
};
use windows_sys::Win32::Storage::FileSystem::*;
type Result<T> = std::result::Result<T, String>;
fn file_pin(host: &str, run: &str) -> Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(Path::new(host).join(format!("SuperHDD-handoff-{run}.vhdx")))
        .map_err(|e| e.to_string())
}
pub fn prepare(run: &str) -> Result<Value> {
    iscsi::target_name(run)?;
    if Path::new(ROOT).join("manifest.json").exists() {
        return Err("An existing protected native owner must be recovered/removed first".into());
    }
    let source_volume = windows::volume_identity("S:")?;
    let source = data_source::inspect_volume(&source_volume)?;
    if source.bus_type != 15
        || source.bytes != 128 * 1024 * 1024
        || source.logical_sector_bytes != 512
        || windows::volume_filesystem(&source_volume.volume_guid)? != "NTFS"
    {
        return Err("Only the disposable 128 MiB NTFS VHDX source is accepted".into());
    }
    let host = windows::volume_guid("D:")?;
    let file = file_pin(&host, run)?;
    let attributes = file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_attributes();
    let dependencies =
        windows::disk_dependencies(&format!("\\\\.\\PhysicalDrive{}", source.number))?;
    let [dependency] = dependencies.as_slice() else {
        return Err("Ambiguous disposable source".into());
    };
    if attributes
        & (FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_COMPRESSED
            | FILE_ATTRIBUTE_ENCRYPTED)
        != 0
        || !dependency.microsoft_provider
        || dependency.device_id != 3
        || dependency.ancestor_level != 0
        || dependency.provider_flags != 0
        || !dependency
            .host_volume
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(host.trim_end_matches('\\'))
        || !dependency
            .relative_path
            .eq_ignore_ascii_case(&format!("\\SuperHDD-handoff-{run}.vhdx"))
    {
        return Err("Fixture dependency does not match the exact generated file".into());
    }
    let parent = windows::volume_identity("M:")?;
    let base_path = Path::new(&parent.volume_guid).join("SuperHDDCache");
    let _base = ProtectedDir::create(&base_path)?;
    let cache = ProtectedDir::create(&base_path.join(run))?;
    let payload = cache.payload("payload.bin", true)?;
    let config = Config {
        schema: 1,
        run: run.into(),
        source,
        source_volume,
        parent,
        capacity_bytes: 32 * 1024 * 1024,
        payload: windows::stable_file_identity(&payload)?,
        source_fully_decrypted: true,
        lab: Some(LabSource {
            file: windows::stable_file_identity(&file)?,
            host_volume: host,
        }),
    };
    config.validate()?;
    let root = ProtectedDir::create(Path::new(ROOT))?;
    let _run = ProtectedDir::create(&Path::new(ROOT).join(run))?;
    let bytes = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
    let mut manifest = root.file("manifest.json", true)?;
    manifest
        .write_all(&bytes)
        .and_then(|_| manifest.sync_all())
        .map_err(|e| e.to_string())?;
    let mut journal = RunJournal::create(root.file("lease.bin", true)?, config_digest(&bytes))?;
    journal.save(Phase::Prepared, Some(config.target()?))?;
    let reservation=superhdd_storage_control::port_reservation::Reservation::create()?;
    let mut port_owner=root.file("port-owner.json",true)?;
    port_owner.write_all(&serde_json::to_vec(&reservation).map_err(|e|e.to_string())?).and_then(|_|port_owner.sync_all()).map_err(|e|e.to_string())?;
    payload
        .set_len(config.capacity_bytes)
        .and_then(|_| payload.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(json!({"prepared":true,"labOnly":true,"active":false,"config":config}))
}
pub fn remove(run: &str) -> Result<Value> {
    iscsi::target_name(run)?;
    let root = ProtectedDir::open(Path::new(ROOT))?;
    let mut manifest = root.read_file("manifest.json")?;
    let mut bytes = Vec::new();
    (&mut manifest)
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Oversized native manifest".into());
    }
    let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    config.validate()?;
    if config.run != run || config.lab.is_none() {
        return Err("Only this disposable lab owner may be removed".into());
    }
    let targets = [iscsi::target_name(run)?, iscsi::managed_target_name(run)?];
    if iscsi::sessions()?
        .iter()
        .any(|s| targets.contains(&s.target))
        || data_source::is_offline(&config.source)?
    {
        return Err(
            "Disconnect owned sessions and restore the original source before removal".into(),
        );
    }
    let cache = ProtectedDir::open(&config.cache_root()?)?;
    let payload = cache.payload("payload.bin", false)?;
    if windows::stable_file_identity(&payload)? != config.payload {
        return Err("Refusing to remove a different payload".into());
    }
    let mut journal = RunJournal::open(root.file("lease.bin", false)?, config_digest(&bytes))?;
    if journal.record().target.as_ref() != Some(&config.target()?)
        && !(journal.record().phase == Phase::Stopped && journal.record().target.is_none()) {
        return Err("Prepared target differs".into());
    }
    journal.save(Phase::Stopped, None)?;
    drop(journal);
    let mut port_owner=root.file("port-owner.json",false)?;
    let mut port_bytes=Vec::new();(&mut port_owner).take(1025).read_to_end(&mut port_bytes).map_err(|e|e.to_string())?;
    if port_bytes.len()>1024{return Err("Invalid endpoint owner size".into());}
    let reservation:superhdd_storage_control::port_reservation::Reservation=serde_json::from_slice(&port_bytes).map_err(|e|e.to_string())?;
    reservation.remove()?;
    protected::remove_file(port_owner)?;
    protected::remove_file(payload)?;
    let cache_path = config.cache_root()?;
    drop(cache);
    fs::remove_dir(cache_path).map_err(|e| e.to_string())?;
    let run_path = Path::new(ROOT).join(run);
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
    fs::remove_dir(run_path).map_err(|e| e.to_string())?;
    drop(manifest);
    for name in ["manifest.json", "lease.bin", "supervisor.lock"] {
        if !Path::new(ROOT).join(name).exists(){continue;}
        protected::remove_file(root.file(name, false)?)?;
    }
    drop(root);
    fs::remove_dir(ROOT).map_err(|e| e.to_string())?;
    Ok(json!({"removed":true,"labOnly":true,"sourceOnline":true}))
}
