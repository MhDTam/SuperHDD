//! Actual v8 kernel proof, restricted to the existing isolated VM and owned files.
#[cfg(windows)]
#[path = "support/paging_workload.rs"]
mod workload;
#[cfg(windows)]
mod proof {
    use serde::Deserialize;
    use serde_json::json;
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::{Path, PathBuf},
    };
    use superhdd_platform::{
        paging_cache::{self, Context},
        paging_driver::Controller,
    };
    use superhdd_storage_control::{
        lab::LabIdentity,
        windows::{volume_identity, OwnedVhd, VerifiedLab},
        ContainerBudget,
    };
    use windows_sys::Win32::{Foundation::CloseHandle, System::Memory::*};
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        source_volume: String,
        container_root: PathBuf,
        budget_bytes: u64,
        isolated_lab: LabIdentity,
    }
    #[repr(align(4096))]
    struct Block([u8; 65536]);
    fn expected(position: usize) -> u8 {
        ((position.wrapping_mul(17) ^ (position >> 9) ^ (position >> 16)) & 255) as u8
    }
    fn fill(block: &mut Block, index: usize, mutated: bool) {
        for (i, b) in block.0.iter_mut().enumerate() {
            let p = index * 65536 + i;
            *b = if mutated && (69632..73728).contains(&p) {
                expected(p) ^ 0xa5
            } else {
                expected(p)
            };
        }
    }
    fn fixture(path: &Path, write: bool, create: bool) -> Result<File> {
        Ok(OpenOptions::new()
            .read(true)
            .write(write)
            .create_new(create)
            .share_mode(3)
            .custom_flags(0x20000000 | if write { 0x80000000 } else { 0 })
            .open(path)?)
    }
    fn reads(file: &mut File, mutated: bool, all: bool) -> Result<()> {
        let mut actual = Block([0; 65536]);
        let mut want = Block([0; 65536]);
        for n in 0..if all { 256 } else { 96 } {
            let block = if all { n } else { (n * 7919) % 256 };
            fill(&mut want, block, mutated);
            file.seek(SeekFrom::Start((block * 65536) as u64))?;
            file.read_exact(&mut actual.0)?;
            if actual.0 != want.0 {
                return Err(format!("Byte oracle mismatch in block {block}").into());
            }
        }
        Ok(())
    }
    fn mapped(path: &Path) -> Result<()> {
        let file = File::open(path)?;
        let map = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READONLY,
                0,
                0,
                std::ptr::null(),
            )
        };
        if map.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let view = unsafe { MapViewOfFile(map, FILE_MAP_READ, 0, 0, 16 * 1024 * 1024) };
        if view.Value.is_null() {
            unsafe { CloseHandle(map) };
            return Err(std::io::Error::last_os_error().into());
        }
        let mut good = true;
        for p in 0..16 * 1024 * 1024 {
            if unsafe { *((view.Value as *const u8).add(p)) } != expected(p) {
                good = false;
                break;
            }
        }
        unsafe {
            UnmapViewOfFile(view);
            CloseHandle(map)
        };
        if !good {
            return Err("Mapped paging read did not preserve source bytes".into());
        }
        Ok(())
    }
    pub fn run(request_path: &Path, report: &Path) -> Result<()> {
        let bytes = std::fs::read(request_path)?;
        if bytes.len() > 8192 {
            return Err("Oversized VM request".into());
        }
        let r: Request = serde_json::from_slice(&bytes)?;
        let lab = VerifiedLab::verify(r.isolated_lab.clone())?;
        lab.validate_root(&r.container_root)?;
        if r.source_volume != "S:" || r.budget_bytes != 128 * 1024 * 1024 {
            return Err("One fixed disposable S/K proof required".into());
        }
        let before = volume_identity("S:")?;
        lab.validate_source_parent(&before)?;
        let driver = Controller::open(true)?;
        if driver.status()?.cache.attached {
            return Err("Existing kernel cache preserved".into());
        }
        let path = PathBuf::from(format!("S:\\transparent-{}.bin", r.isolated_lab.run_id));
        let mut result = json!({"passed":false,"protocolVersion":8,"sourceRemainedMounted":false,"physicalHostAccelerationActive":false});
        let mut session = None;
        let mut created = false;
        let outcome = (|| -> Result<()> {
            result["stage"] = json!("seed-fixture");
            {
                let mut file = fixture(&path, true, true)?;
                created = true;
                let mut block = Block([0; 65536]);
                for n in 0..256 {
                    fill(&mut block, n, false);
                    file.write_all(&block.0)?;
                }
                file.sync_all()?;
            }
            result["stage"] = json!("capture-context");
            let context = Context::capture("S:", &r.container_root, Some(r.isolated_lab.clone()))?;
            result["stage"] = json!("create-owned-container");
            let _private = superhdd_storage_control::windows::protected::ProtectedDir::create(
                &r.container_root,
            )?;
            session = Some(paging_cache::open(
                context,
                &r.container_root,
                Some(ContainerBudget::new(r.budget_bytes)?),
            )?);
            result["stage"] = json!("configure-kernel");
            let s = session.as_mut().unwrap();
            s.start()?;
            let active = driver.status()?;
            if !active.cache.attached
                || active.cache.cache_disabled
                || active.paging_files == 0
                || active.paging_passthrough
            {
                return Err(
                    "Kernel did not confirm active cache alongside an actual page file".into(),
                );
            }
            result["active"] = serde_json::to_value(&active)?;
            result["stage"] = json!("read-oracles");
            {
                let mut file = fixture(&path, false, false)?;
                reads(&mut file, false, true)?;
            }
            let cold = driver.status()?;
            {
                let mut file = fixture(&path, false, false)?;
                reads(&mut file, false, true)?;
            }
            let warm = driver.status()?;
            let ssd = warm
                .cache
                .ssd_read_bytes
                .checked_sub(cold.cache.ssd_read_bytes)
                .ok_or("Counter reset")?;
            if warm.cache.hits <= cold.cache.hits || ssd < 6 * 1024 * 1024 {
                return Err("Actual warm SSD hits not demonstrated".into());
            }
            result["warmDelta"] = json!({"ssdReadBytes":ssd,"hits":warm.cache.hits-cold.cache.hits,"sourceReadBytes":warm.cache.source_read_bytes-cold.cache.source_read_bytes});
            let started = std::time::Instant::now();
            mapped(&path)?;
            let mapped_status = driver.status()?;
            let paging_hits = mapped_status.paging_hits - warm.paging_hits;
            result["mappedElapsedMicros"] = json!(started.elapsed().as_micros());
            result["mappedPagingHits"] = json!(paging_hits);
            result["mappedSsdReadBytes"] =
                json!(mapped_status.cache.ssd_read_bytes - warm.cache.ssd_read_bytes);
            result["mappedSourceReadBytes"] =
                json!(mapped_status.cache.source_read_bytes - warm.cache.source_read_bytes);
            result["mappedBytesVerified"] = json!(true);
            result["mappedPagingBypassDelta"] =
                json!(mapped_status.paging_bypasses - warm.paging_bypasses);
            result["sourcePagingReadsObserved"] = json!(mapped_status.paging_bypasses);
            result["stage"] = json!("buffered-mapped-pressure-workload");
            let workload = crate::workload::run(&driver, s, &r.isolated_lab.run_id)?;
            let workload_passed = workload["passed"] == true;
            result["applicationWorkload"] = workload;
            if !workload_passed {
                return Err("Application workload did not demonstrate both buffered and mapped paging SSD hits without errors".into());
            }
            result["stage"] = json!("write-oracle");
            {
                let mut file = fixture(&path, true, false)?;
                let mut block = Block([0; 65536]);
                fill(&mut block, 1, true);
                file.seek(SeekFrom::Start(69632))?;
                file.write_all(&block.0[4096..8192])?;
                file.sync_all()?;
            }
            {
                let mut file = fixture(&path, false, false)?;
                reads(&mut file, true, true)?;
            }
            result["durableMutationOracle"] = json!(true);
            s.pause(true)?;
            let paused = driver.status()?;
            {
                let mut file = fixture(&path, false, false)?;
                reads(&mut file, true, false)?;
            }
            let after = driver.status()?;
            if after.cache.ssd_read_bytes != paused.cache.ssd_read_bytes
                || after.cache.source_read_bytes != paused.cache.source_read_bytes
            {
                return Err("Paused reads unexpectedly entered the cache worker".into());
            }
            s.pause(false)?;
            s.clear()?;
            s.stop()?;
            result["pauseClearStopVerified"] = json!(true);
            if driver.status()?.cache.attached
                || serde_json::to_value(volume_identity("S:")?)? != serde_json::to_value(&before)?
            {
                return Err("Source identity changed or kernel did not detach".into());
            }
            {
                let mut file = fixture(&path, false, false)?;
                reads(&mut file, true, true)?;
            }
            result["sourceRemainedMounted"] = json!(true);
            result["passed"] = json!(true);
            Ok(())
        })();
        if let Err(error) = &outcome {
            result["error"] = json!(error.to_string());
            result["configurationDiagnostic"] = json!(driver.diagnostic().ok());
        }
        let cleanup = (|| -> Result<()> {
            if let Some(s) = session.as_mut() {
                s.stop()?;
            }
            drop(session.take());
            if driver.status()?.cache.attached {
                return Err("Kernel backing preserved while attached".into());
            }
            if r.container_root.join("owner.json").exists() {
                OwnedVhd::reopen_in_lab_service(&r.container_root, lab)?.remove()?;
            }
            if created {
                std::fs::remove_file(&path)?;
            }
            result["ownedFixturesRemoved"] = json!(true);
            Ok(())
        })();
        if let Err(error) = &cleanup {
            result["passed"] = json!(false);
            result["cleanupError"] = json!(error.to_string());
        }
        std::fs::write(report, serde_json::to_vec_pretty(&result)?)?;
        outcome?;
        cleanup?;
        Ok(())
    }
    pub fn main() -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() == 1 && args[0] == "status" {
            println!(
                "{}",
                serde_json::to_string(&Controller::open(true)?.status()?)?
            );
            return Ok(());
        }
        if args.len() != 2 {
            return Err("Usage: paging_session_probe REQUEST REPORT".into());
        }
        run(Path::new(&args[0]), Path::new(&args[1]))
    }
}
#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    proof::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows isolated VM required");
    std::process::exit(1);
}
