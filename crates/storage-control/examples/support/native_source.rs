//! Disposable fixture guard only. This executable cannot enroll the real D disk.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::fs::MetadataExt,
    path::PathBuf,
};
use superhdd_storage_control::{
    data_source::{self, DiskIdentity},
    iscsi, windows, VolumeIdentity,
};
type Result<T> = std::result::Result<T, String>;
fn root(run: &str) -> Result<PathBuf> {
    iscsi::target_name(run)?;
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/runtime")
        .join(format!("iscsi-local-{run}")))
}
fn owned_source(run: &str, number: u32) -> Result<()> {
    let source = PathBuf::from(format!("D:\\SuperHDD-handoff-{run}.vhdx"));
    let metadata = fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
    let saved: serde_json::Value = serde_json::from_slice(
        &fs::read(root(run)?.join("handoff-source-owner.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || saved["runId"] != run
        || saved["created"] != metadata.creation_time()
        || saved["sourceBytes"] != 128 * 1024 * 1024u64
    {
        return Err("Owned VHDX marker changed".into());
    }
    let dependencies = windows::disk_dependencies(&format!("\\\\.\\PhysicalDrive{number}"))?;
    let [dependency] = dependencies.as_slice() else {
        return Err("Ambiguous owned VHDX dependencies".into());
    };
    if !dependency.microsoft_provider
        || dependency.device_id != 3
        || dependency.ancestor_level != 0
        || dependency.provider_flags != 0
        || !dependency
            .host_volume
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(windows::volume_guid("D:")?.trim_end_matches('\\'))
        || !dependency
            .relative_path
            .eq_ignore_ascii_case(&format!("\\SuperHDD-handoff-{run}.vhdx"))
    {
        return Err("Physical source is never accepted by this fixture".into());
    }
    Ok(())
}
fn target_number(run: &str, managed: bool) -> Result<u32> {
    let target = if managed {
        iscsi::managed_target_name(run)?
    } else {
        iscsi::target_name(run)?
    };
    let sessions: Vec<_> = iscsi::sessions()?
        .into_iter()
        .filter(|s| s.target == target)
        .collect();
    let [session] = sessions.as_slice() else {
        return Err("Require exactly one owned target session".into());
    };
    let devices = iscsi::devices(session.id)?;
    let [device] = devices.as_slice() else {
        return Err("Require exactly one owned target device".into());
    };
    if device.device_type != 7 {
        return Err("Owned target is not a disk".into());
    }
    Ok(device.disk_number)
}
fn save(run: &str, name: &str, identity: &DiskIdentity) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root(run)?.join(name))
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(identity).map_err(|e| e.to_string())?)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn load(run: &str, name: &str) -> Result<DiskIdentity> {
    serde_json::from_slice(&fs::read(root(run)?.join(name)).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn guard_source(run: &str, identity: &DiskIdentity) -> Result<()> {
    owned_source(run, identity.number)?;
    if identity.bus_type != 15 || identity.bytes != 128 * 1024 * 1024 {
        return Err("Only fixed-size owned virtual source supported".into());
    }
    Ok(())
}
fn target_identity(run: &str, number: u32) -> Result<DiskIdentity> {
    let mut identity = load(run, "native-source.json")?;
    identity.number = number;
    identity.bus_type = 9;
    identity.serial = run.into();
    Ok(identity)
}
pub fn run(operation: &str, run: &str, mount: Option<&str>) -> Result<serde_json::Value> {
    let managed = operation.ends_with("-managed");
    let operation = operation.strip_suffix("-managed").unwrap_or(operation);
    match operation {
        "lock-source" | "lock-target" => {
            let volume: VolumeIdentity = windows::volume_identity(mount.ok_or("Volume required")?)?;
            let identity = data_source::inspect_volume(&volume)?;
            let source = operation == "lock-source";
            if source {
                guard_source(run, &identity)?;
            } else {
                if identity != target_identity(run, target_number(run, managed)?)? {
                    return Err("Projected disk does not match original GPT layout".into());
                }
            }
            // Prove open files prevent handoff before taking the clean lock.
            let held = OpenOptions::new()
                .read(true)
                .open(format!("{}fixture.bin", volume.volume_guid))
                .map_err(|e| e.to_string())?;
            if data_source::lock_and_offline(&volume, &identity).is_ok() {
                return Err("Open fixture unexpectedly allowed volume handoff".into());
            }
            drop(held);
            if source {
                save(run, "native-source.json", &identity)?;
            }
            data_source::lock_and_offline(&volume, &identity)?;
            Ok(
                serde_json::json!({"offline":true,"persisted":false,"openFilesRefused":true,"identity":identity}),
            )
        }
        "restore-source" => {
            let identity = load(run, "native-source.json")?;
            guard_source(run, &identity)?;
            let targets = [iscsi::target_name(run)?, iscsi::managed_target_name(run)?];
            if iscsi::sessions()?
                .iter()
                .any(|s| targets.contains(&s.target))
            {
                return Err("Cannot restore source while target session remains".into());
            }
            data_source::restore_online(&identity)?;
            Ok(serde_json::json!({"online":true,"persisted":false,"identity":identity}))
        }
        "restore-target" => {
            let identity = target_identity(run, target_number(run, managed)?)?;
            data_source::restore_online(&identity)?;
            Ok(serde_json::json!({"online":true,"persisted":false,"identity":identity}))
        }
        _ => Err("Unsupported source handoff operation".into()),
    }
}
