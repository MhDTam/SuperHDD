//! Actual native-v4 -> authenticated service -> desktop-telemetry contract proof.
//! Requires the already provisioned, signed disposable Super HDD VM.
#[cfg(windows)]
fn run(root: &std::path::Path, report: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        fs::OpenOptions,
        io::{Read, Seek, SeekFrom, Write},
        os::windows::fs::OpenOptionsExt,
    };
    use superhdd_platform::{driver, ipc, raw_lab::Controller};
    type Error = Box<dyn std::error::Error>;
    let identity=superhdd_platform::hidden_command("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$c=Get-CimInstance Win32_ComputerSystem;if($c.Manufacturer -eq 'Microsoft Corporation' -and $c.Model -eq 'Virtual Machine' -and $env:COMPUTERNAME -eq 'SHDD-LAB'){exit 0}else{exit 9}"]).status()?;
    if !identity.success() {
        return Err("Native bridge probe refused: disposable Super HDD VM only".into());
    }
    let text = root.to_str().ok_or("Invalid fixture root")?;
    if !text
        .strip_prefix("R:\\superhdd-native-bridge-")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Use the new owned R:\\superhdd-native-bridge-<32hex> fixture".into());
    }
    let controller = Controller::open()?;
    let before = controller.status()?;
    if !before.cache.attached
        || before.cache.paused
        || before.cache.cache_disabled
        || before.pnp_state != 1
    {
        return Err("Require the configured disposable v4 session".into());
    }
    let volume=superhdd_platform::hidden_command("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","$p=Get-Partition -DriveLetter R -ErrorAction Stop;$d=$p|Get-Disk;$v=$p|Get-Volume;if($d.BusType -eq 'File Backed Virtual' -and -not $d.IsBoot -and -not $d.IsSystem -and $v.FileSystemLabel -eq 'SHDD-Lab-Source'){$d.Number}else{exit 9}"]).output()?;
    if !volume.status.success()
        || String::from_utf8_lossy(&volume.stdout)
            .trim()
            .parse::<u32>()?
            != before.source_disk_number
    {
        return Err("Source volume identity mismatch".into());
    }
    struct Aligned(*mut u8);
    impl Aligned {
        fn new() -> Self {
            let p = unsafe { alloc_zeroed(Layout::from_size_align(65536, 4096).unwrap()) };
            if p.is_null() {
                std::alloc::handle_alloc_error(Layout::from_size_align(65536, 4096).unwrap())
            }
            Self(p)
        }
        fn bytes(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.0, 65536) }
        }
    }
    impl Drop for Aligned {
        fn drop(&mut self) {
            unsafe { dealloc(self.0, Layout::from_size_align(65536, 4096).unwrap()) }
        }
    }
    let check = |stats: &driver::Telemetry, attached: bool| -> Result<(), Error> {
        let native = stats
            .native_volume
            .as_ref()
            .ok_or("Native diagnostics missing from bridge")?;
        if stats.protocol_version != 4
            || stats.attached != attached
            || (attached
                && (native.source_disk_number != Some(before.source_disk_number)
                    || native.backing_disk_number != Some(before.backing_disk_number)
                    || !native.store_locked
                    || native.allocated_bytes != before.allocated_bytes))
            || (!attached
                && (native.source_disk_number.is_some()
                    || native.backing_disk_number.is_some()
                    || native.store_locked
                    || native.allocated_bytes != 0))
        {
            return Err(
                "Incorrect bridge protocol, source/cache identity or lifecycle state".into(),
            );
        }
        Ok(())
    };
    let direct_before = driver::telemetry()?;
    check(&direct_before, true)?;
    let service_before = ipc::telemetry()?;
    check(&service_before, true)?;
    std::fs::create_dir(root)?;
    let fixture = root.join("owned-fixture.bin");
    let mut expected = Aligned::new();
    let mut actual = Aligned::new();
    for (index, byte) in expected.bytes().iter_mut().enumerate() {
        *byte = ((index * 31 + index / 13) % 251) as u8;
    }
    let mut digest = Sha256::new();
    let test = (|| -> Result<serde_json::Value, Error> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(0x20000000 | 0x10000000)
            .open(&fixture)?;
        for _ in 0..4 {
            file.write_all(expected.bytes())?;
        }
        file.sync_all()?;
        for _ in 0..3 {
            file.seek(SeekFrom::Start(0))?;
            for _ in 0..4 {
                file.read_exact(actual.bytes())?;
                if actual.bytes() != expected.bytes() {
                    return Err("Native ordinary-file data mismatch".into());
                }
                digest.update(actual.bytes());
            }
        }
        drop(file);
        let direct_after = driver::telemetry()?;
        check(&direct_after, true)?;
        let service_after = ipc::telemetry()?;
        check(&service_after, true)?;
        if service_after.hits <= service_before.hits
            || service_after.ssd_read_bytes <= service_before.ssd_read_bytes
        {
            return Err("No actual SSD read reached the desktop service telemetry route".into());
        }
        controller.disable()?;
        let disabled = ipc::telemetry()?;
        check(&disabled, false)?;
        Ok(
            serde_json::json!({"passed":true,"scope":"Own signed-v4 VM driver, ordinary noncached byte-checked reads, the exact desktop telemetry function and SCM-PID-authenticated read-only service pipe. Not physical-host activation.",
            "directReadOnlyBefore":direct_before,"serviceBefore":service_before,"directReadOnlyAfter":direct_after,"serviceAfter":service_after,"disabled":disabled,
            "checks":{"nativeProtocol4":true,"readOnlyDeviceHandle":true,"servicePeerAuthenticated":true,"sourceAndCacheIdentities":true,"ordinaryFileBytes":true,"ssdHitsVisibleThroughService":true,"disableStateVisible":true},
            "readOperations":12,"readBytes":12*65536,"readSha256":format!("{:x}",digest.finalize())}),
        )
    })();
    let _ = controller.disable();
    let cleanup = (|| -> Result<(), Error> {
        std::fs::remove_file(&fixture)?;
        std::fs::remove_dir(root)?;
        Ok(())
    })();
    let mut value = match &test {
        Ok(value) => value.clone(),
        Err(error) => serde_json::json!({"passed":false,"error":error.to_string()}),
    };
    value["fixtureRemoved"] = serde_json::json!(cleanup.is_ok());
    if cleanup.is_err() {
        value["passed"] = serde_json::json!(false);
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)?
        .write_all(&serde_json::to_vec_pretty(&value)?)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    test?;
    cleanup?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("native_bridge_probe NEW_OWNED_R_FIXTURE REPORT.json\nDisposable signed-driver Super HDD VM only; requires the running native service and configured v4 lab session.");
        return Ok(());
    }
    #[cfg(windows)]
    if let [root, report] = args.as_slice() {
        return run(std::path::Path::new(root), std::path::Path::new(report));
    }
    Err("Windows VM and two explicit arguments required".into())
}
