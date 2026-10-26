//! Disposable SSD-container lifecycle proof. Never loads a kernel filter.
#[cfg(windows)]
fn run(
    command: &str,
    root: &std::path::Path,
    report: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs::OpenOptions, io::Write};
    use superhdd_storage_control::{journal::Phase, windows::OwnedVhd, ContainerBudget};
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Missing fixture name")?;
    if !name
        .strip_prefix("cache-lifecycle-")
        .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        || !root.is_absolute()
        || report.exists()
        || root.parent().ok_or("No fixture parent")?.canonicalize()?
            != std::path::Path::new("M:\\Codex\\Super HDD\\artifacts\\runtime").canonicalize()?
        || !matches!(command, "prepare" | "hold" | "recover")
    {
        return Err("Use prepare/hold/recover, an owned project runtime cache-lifecycle-<32hex> folder, and a new report.".into());
    }
    let before = unsafe { GetLogicalDrives() };
    let mut container = if command == "recover" {
        OwnedVhd::reopen(root)?
    } else {
        std::fs::create_dir(root)?;
        OwnedVhd::create(root, ContainerBudget::new(256 * 1024 * 1024)?)?
    };
    let owned_path = container.path().to_path_buf();
    let budget = container.budget;
    let file_bytes = container.file_bytes;
    let result = (|| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let physical = container.attach()?;
        let raw = if command == "recover" {
            container.raw_volume()?
        } else {
            container.initialize_raw()?
        };
        let same = container.raw_volume()?;
        let no_reinitialize = container.initialize_raw().is_err();
        let no_second_owner = OwnedVhd::reopen(root).is_err();
        let after = unsafe { GetLogicalDrives() };
        if before != after
            || same.device_path != raw.device_path
            || !no_reinitialize
            || !no_second_owner
            || container.phase() != Phase::RawReady
        {
            return Err("RAW volume identity, drive-letter, initialization or exclusive-owner check failed.".into());
        }
        Ok(
            serde_json::json!({"passed":true,"command":command,"physicalDisk":physical,"rawVolume":raw,
            "instance":container.instance(),"parent":container.parent,"fileBytes":file_bytes,"budget":budget,"ownedPath":owned_path,
            "checks":{"rawHeaderVerified":true,"noDriveLetter":before==after,"reinitializationRejected":no_reinitialize,"secondOwnerRejected":no_second_owner},
            "scope":"Owned new M: SSD VHDX only; no source-volume writes, driver loading or system acceleration."}),
        )
    })();
    let mut value = match result {
        Ok(value) => value,
        Err(error) => {
            let cleanup = container.remove();
            let value = serde_json::json!({"passed":false,"command":command,"error":error.to_string(),"cleanupError":cleanup.err(),"ownedPath":owned_path});
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(report)?
                .write_all(&serde_json::to_vec_pretty(&value)?)?;
            return Err(error);
        }
    };
    if command == "hold" {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report)?;
        file.write_all(&serde_json::to_vec_pretty(&value)?)?;
        file.sync_all()?;
        // The harness kills this exact owned process after the durable checkpoint.
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if command == "recover" {
        container.remove()?;
        if owned_path.exists() || root.join("owner.json").exists() {
            return Err("Owned container cleanup incomplete.".into());
        }
        std::fs::remove_dir(root)?;
        value["removed"] = serde_json::json!(true);
    } else {
        container.detach()?;
        drop(container);
        value["preservedForRecovery"] = serde_json::json!(true);
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?
        .write_all(&serde_json::to_vec_pretty(&value)?)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("container_lifecycle_probe prepare|hold|recover OWNED_RUNTIME_FOLDER NEW_REPORT.json\nCreates/reopens only a new 256 MiB-budget SSD VHDX, its RAW partition and owned v4 marker. No source disk or kernel driver is changed.");
        return Ok(());
    }
    #[cfg(windows)]
    if let [command, root, report] = args.as_slice() {
        return run(
            command,
            std::path::Path::new(root),
            std::path::Path::new(report),
        );
    }
    Err("Windows and three arguments are required.".into())
}
