//! Real, bounded SSD VHDX creation and backing-dependency proof. No source writes.
#[cfg(windows)]
fn run(root: &std::path::Path, report: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs::OpenOptions, io::Write};
    use superhdd_storage_control::{
        validate_separation,
        windows::{disk_dependencies, volume_identity, OwnedVhd},
        ContainerBudget,
    };
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid probe root")?;
    if !name
        .strip_prefix("cache-control-")
        .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        || root.exists()
        || report.exists()
        || !root.is_absolute()
    {
        return Err(
            "Use a new absolute cache-control-<32hex> directory and a new report file.".into(),
        );
    }
    let parent = root.parent().ok_or("Missing parent")?.canonicalize()?;
    let expected_parent =
        std::path::Path::new("M:\\Codex\\Super HDD\\artifacts\\runtime").canonicalize()?;
    if parent != expected_parent {
        return Err("Host probe only accepts the owned project runtime folder on M SSD.".into());
    }
    let source = volume_identity("D:")?;
    let ssd = volume_identity("M:")?;
    if source.disk_number == ssd.disk_number
        || source.incurs_seek_penalty != Some(true)
        || ssd.incurs_seek_penalty != Some(false)
    {
        return Err("Expected physically separate D HDD and M SSD.".into());
    }
    let other_source = volume_identity("E:");
    std::fs::create_dir(root)?;
    let before = unsafe { GetLogicalDrives() };
    let mut container = OwnedVhd::create(root, ContainerBudget::new(256 * 1024 * 1024)?)?;
    let owned_path = container.path().to_path_buf();
    let file_bytes = container.file_bytes;
    let budget = container.budget;
    let actual_parent = container.parent.clone();
    let proof = (|| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let physical = container.attach()?;
        let dependencies = disk_dependencies(&physical)?;
        let result = validate_separation(&source, &actual_parent, &dependencies);
        let e_result = other_source
            .as_ref()
            .map(|source| validate_separation(source, &actual_parent, &dependencies));
        let after = unsafe { GetLogicalDrives() };
        if before != after {
            return Err("Container unexpectedly acquired a drive letter.".into());
        }
        let mut recursive = dependencies.clone();
        if let Some(entry) = recursive.first_mut() {
            entry.host_volume = source.volume_guid.clone();
        }
        let recursive_rejected = validate_separation(&source, &actual_parent, &recursive).is_err();
        let mut report = serde_json::json!({"sourceD":source,"selectedSsd":ssd,"actualParent":actual_parent,
            "virtualDisk":physical,"dependencies":dependencies,"logicalDriveMaskBefore":before,"logicalDriveMaskAfter":after,
            "checks":{"sourceHddAndCacheSsd":result.is_ok(),"noNewDriveLetter":true,"recursiveBackingRejected":recursive_rejected},
            "sourceE":match &other_source{Ok(source)=>serde_json::json!({"identity":source,"topologyPassed":e_result.as_ref().is_ok_and(|r|r.is_ok())}),Err(error)=>serde_json::json!({"queryError":error})}});
        if let Err(error) = result {
            report["validationError"] = serde_json::json!(error);
        }
        report["passed"] = serde_json::json!(
            report["checks"]["sourceHddAndCacheSsd"] == true && recursive_rejected
        );
        Ok(report)
    })();
    let cleanup = container.remove();
    let mut value = match proof {
        Ok(value) => value,
        Err(error) => serde_json::json!({"passed":false,"error":error.to_string()}),
    };
    value["cleanup"] = serde_json::json!({"nativeDetachAndVerifiedDeletion":cleanup.is_ok(),"containerAbsent":!owned_path.exists(),"ownershipJournalAbsent":!root.join("owner.json").exists()});
    if let Err(error) = cleanup {
        value["cleanupError"] = serde_json::json!(error);
        value["passed"] = serde_json::json!(false);
    }
    if !owned_path.exists() && !root.join("owner.json").exists() {
        std::fs::remove_dir(root)?;
    }
    value["scope"]=serde_json::json!("Actual Windows VirtDisk APIs and real physical D/M identities; new fixed-size SSD VHDX only. No source-volume writes, filter enrollment, driver installation, host-security changes or global acceleration.");
    value["budget"] = serde_json::json!(budget);
    value["containerFileBytes"] = serde_json::json!(file_bytes);
    value["ownedPath"] = serde_json::json!(owned_path);
    value["rootRemoved"] = serde_json::json!(!root.exists());
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?
        .write_all(&serde_json::to_vec_pretty(&value)?)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    if value["passed"] != true {
        return Err("Container topology proof did not pass; inspect its recorded result.".into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("container_probe NEW_M_RUNTIME_DIRECTORY NEW_REPORT.json\nCreates, queries, detaches and removes one owned 256 MiB-budget VHDX. Requires administrator rights; never attaches our kernel filter.");
        return Ok(());
    }
    #[cfg(windows)]
    if let [root, report] = args.as_slice() {
        return run(std::path::Path::new(root), std::path::Path::new(report));
    }
    Err("Windows and two explicit paths required.".into())
}
