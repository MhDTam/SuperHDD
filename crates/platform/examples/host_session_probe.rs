//! Windows-host developer session, restricted to a newly owned file-backed VHD.
#[cfg(windows)]
mod proof {
    use serde::Deserialize;
    use serde_json::{json, Value};
    use std::{
        fs::{File, OpenOptions},
        io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
        os::windows::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Stdio},
        sync::mpsc::{self, Receiver},
        time::Duration,
    };
    use superhdd_platform::raw_lab::Controller;
    use superhdd_storage_control::windows::{
        disk_dependencies, volume_filesystem, volume_identity, OwnedVhd,
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        source_volume: String,
        container_root: PathBuf,
        budget_bytes: Option<u64>,
        acknowledged_disposable_source: bool,
        isolated_lab: Option<Value>,
    }
    struct Owner {
        child: Child,
        input: Option<ChildStdin>,
        replies: Receiver<Value>,
    }
    impl Owner {
        fn start(tool: &Path, request: &Path, log: &Path) -> Result<Self> {
            let mut child =
                superhdd_platform::hidden_command(tool.to_str().ok_or("Invalid helper")?)
                    .arg("--lab-cache-session")
                    .arg(request)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(File::create_new(log)?)
                    .spawn()?;
            let input = child.stdin.take();
            let output = child.stdout.take().ok_or("No owner output")?;
            let (sender, replies) = mpsc::channel();
            std::thread::spawn(move || {
                let mut stream = BufReader::new(output);
                loop {
                    let mut bytes = Vec::new();
                    match Read::by_ref(&mut stream)
                        .take(4097)
                        .read_until(b'\n', &mut bytes)
                    {
                        Ok(0) | Err(_) => break,
                        Ok(_) if bytes.len() <= 4096 => {
                            if let Ok(value) = serde_json::from_slice(&bytes) {
                                if sender.send(value).is_err() {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            });
            Ok(Self {
                child,
                input,
                replies,
            })
        }
        fn reply(&self, phase: &str) -> Result<Value> {
            let value = self.replies.recv_timeout(Duration::from_secs(30))?;
            if value["ok"] != true || value["phase"] != phase {
                return Err(format!("Owner did not reach {phase}: {value}").into());
            }
            Ok(value)
        }
        fn command(&mut self, command: &str, phase: &str) -> Result<Value> {
            let stream = self.input.as_mut().ok_or("Owner input closed")?;
            writeln!(stream, "{command}")?;
            stream.flush()?;
            self.reply(phase)
        }
        fn stop(&mut self) -> Result<()> {
            self.command("stop", "stopped")?;
            self.input.take();
            for _ in 0..100 {
                if let Some(status) = self.child.try_wait()? {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err("Owner exit failed".into())
                    };
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err("Owner stop pending; preserve the mounted test files".into())
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            self.input.take();
        } // EOF requests transactional stop; never kill I/O.
    }
    #[repr(align(4096))]
    struct Block([u8; 65536]);
    fn pattern(block: &mut Block, index: usize, revision: usize) {
        for (i, byte) in block.0.iter_mut().enumerate() {
            *byte = ((i * 31 + i / 13 + index * 19 + revision * 71) % 251) as u8;
        }
    }
    fn reads(file: &mut File, revision: usize) -> Result<()> {
        let mut actual = Box::new(Block([0; 65536]));
        let mut expected = Box::new(Block([0; 65536]));
        file.seek(SeekFrom::Start(0))?;
        for index in 0..4 {
            pattern(&mut expected, index, if index == 0 { revision } else { 0 });
            file.read_exact(&mut actual.0)?;
            if actual.0 != expected.0 {
                return Err("Incorrect host test bytes".into());
            }
        }
        Ok(())
    }
    fn validate_root(root: &Path) -> Result<&str> {
        let run_root = root.parent().ok_or("No run root")?;
        let run = run_root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("host-local-"))
            .ok_or("Not a host test directory")?;
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../artifacts/runtime")
            .canonicalize()?;
        if run.len() != 32
            || !run.bytes().all(|b| b.is_ascii_hexdigit())
            || run_root.parent().ok_or("No runtime root")?.canonicalize()? != runtime
            || root.file_name().is_none_or(|n| n != "cache")
        {
            return Err("Only a host-test owned cache directory is accepted".into());
        }
        Ok(run)
    }
    pub fn remove_owned_cache(root: &Path) -> Result<()> {
        validate_root(root)?;
        if superhdd_platform::raw_lab::try_telemetry()?
            .is_some_and(|s| s.cache.attached || s.store_locked)
        {
            return Err("Kernel session still active; preserve backing".into());
        }
        // Native ownership, file ID, VHD ID, SSD identity and exclusive journal
        // checks still apply. No forced detach or kernel disable is sent.
        if root.join("owner.json").exists() {
            OwnedVhd::reopen(root)?.remove()?;
        }
        std::fs::remove_dir(root)?; // Empty directory only.
        Ok(())
    }
    pub fn run(tool: &Path, request_file: &Path, report: &Path) -> Result<()> {
        let r: Request = serde_json::from_reader(File::open(request_file)?)?;
        validate_root(&r.container_root)?;
        let run_root = r.container_root.parent().ok_or("No run root")?;
        let run = run_root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("host-local-"))
            .ok_or("Not a host test directory")?;
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../artifacts/runtime")
            .canonicalize()?;
        if run.len() != 32
            || !run.bytes().all(|b| b.is_ascii_hexdigit())
            || run_root.parent().ok_or("No runtime root")?.canonicalize()? != runtime
            || r.container_root.file_name().is_none_or(|n| n != "cache")
            || r.source_volume != "S:"
            || r.budget_bytes != Some(256 * 1024 * 1024)
            || !r.acknowledged_disposable_source
            || r.isolated_lab.is_some()
            || request_file.parent() != Some(run_root)
            || report.parent() != Some(run_root)
            || report.exists()
        {
            return Err("Require the exact new host-owned virtual test request".into());
        }
        let expected_tool = superhdd_platform::cache_runtime::install_root()?
            .parent()
            .ok_or("No Program Files")?
            .join("Super HDD Host Test/SuperHDDService.exe");
        if tool.canonicalize()? != expected_tool.canonicalize()? {
            return Err("Use the protected host-test helper".into());
        }
        let source = volume_identity("S:")?;
        if source.bus_type != 15 || source.removable || volume_filesystem("S:")? != "NTFS" {
            return Err("A disposable file-backed source is required".into());
        }
        let dependencies = disk_dependencies(&format!(r"\\.\PhysicalDrive{}", source.disk_number))?;
        let [dependency] = dependencies.as_slice() else {
            return Err("Ambiguous source ancestry".into());
        };
        let parent = volume_identity("D:")?;
        if !dependency.microsoft_provider
            || dependency.device_id != 3
            || dependency.ancestor_level != 0
            || dependency.flags & 2 == 0
            || dependency.provider_flags != 0
            || !dependency
                .host_volume
                .trim_end_matches('\\')
                .eq_ignore_ascii_case(parent.volume_guid.trim_end_matches('\\'))
            || dependency.relative_path.trim_start_matches('\\')
                != format!("SuperHDD-LocalSource-{run}.vhdx")
        {
            return Err("Source is not the new, fixed host-test VHDX on D".into());
        }
        let controller = Controller::open()?;
        if controller.status()?.cache.attached {
            return Err("An existing session is active".into());
        }
        let fixture = PathBuf::from(format!(r"S:\host-fixture-{run}.bin"));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(0x20000000 | 0x10000000)
            .open(&fixture)?;
        let mut block = Box::new(Block([0; 65536]));
        for i in 0..4 {
            pattern(&mut block, i, 0);
            file.write_all(&block.0)?;
        }
        file.sync_all()?;
        let test = (|| -> Result<Value> {
            let mut owner = Owner::start(tool, request_file, &run_root.join("owner.log"))?;
            let active = owner.reply("active")?;
            let target = &active["target"];
            let before = controller.status()?;
            if !before.cache.attached
                || !before.store_locked
                || before.cache.cache_disabled
                || before.pnp_state != 1
                || target["sourceDisk"].as_u64() != Some(u64::from(source.disk_number))
                || target["cacheDisk"].as_u64() != Some(u64::from(before.backing_disk_number))
            {
                return Err("Kernel did not confirm the service-owned source and cache".into());
            }
            for _ in 0..3 {
                reads(&mut file, 0)?;
            }
            let warm = controller.status()?;
            if warm.source_disk_number != source.disk_number
                || warm.cache.hits <= before.cache.hits
                || warm.cache.ssd_read_bytes <= before.cache.ssd_read_bytes
                || warm.cache.errors != 0
            {
                return Err("No host kernel SSD hits".into());
            }
            owner.command("pause", "paused")?;
            let paused = controller.status()?;
            reads(&mut file, 0)?;
            if !paused.cache.paused
                || controller.status()?.cache.ssd_read_bytes != paused.cache.ssd_read_bytes
            {
                return Err("Pause did not bypass cache".into());
            }
            owner.command("clear", "paused")?;
            let cleared = controller.status()?;
            if cleared.cache.generation <= paused.cache.generation {
                return Err("Clear did not invalidate cached records".into());
            }
            owner.command("resume", "active")?;
            let before_refill = controller.status()?;
            reads(&mut file, 0)?;
            let refilled = controller.status()?;
            if refilled
                .cache
                .fill_bytes
                .saturating_sub(before_refill.cache.fill_bytes)
                < 4 * 65536
                || refilled
                    .cache
                    .source_read_bytes
                    .saturating_sub(before_refill.cache.source_read_bytes)
                    < 4 * 65536
            {
                return Err("Clear did not force source reads and cache refill".into());
            }
            file.seek(SeekFrom::Start(0))?;
            pattern(&mut block, 0, 1);
            file.write_all(&block.0)?;
            file.sync_all()?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            owner.stop()?;
            let stopped = controller.status()?;
            if stopped.cache.attached || stopped.store_locked {
                return Err("Kernel retained the cache".into());
            }
            let owned = OwnedVhd::reopen(&r.container_root)?;
            let instance = owned.instance();
            drop(owned);
            let mut reopen: Value = serde_json::from_reader(File::open(request_file)?)?;
            reopen["budgetBytes"] = Value::Null;
            let reopen_file = run_root.join("reopen.json");
            File::create_new(&reopen_file)?.write_all(&serde_json::to_vec(&reopen)?)?;
            let mut second = Owner::start(tool, &reopen_file, &run_root.join("reopen.log"))?;
            second.reply("active")?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            let revalidated = controller.status()?;
            if revalidated.revalidated_records == 0 || revalidated.cache.errors != 0 {
                return Err("Host reopen validation failed".into());
            }
            second.stop()?;
            let same = OwnedVhd::reopen(&r.container_root)?;
            if same.instance() != instance {
                return Err("Owned container changed".into());
            }
            same.remove()?;
            Ok(
                json!({"passed":true,"checks":{"ordinaryBytes":true,"actualKernelSsdHits":true,"pauseBypass":true,"clearResume":true,"writeFreshness":true,"normalStop":true,"sameContainerReopen":true,"sourceRevalidation":true},"before":before,"warm":warm,"revalidated":revalidated}),
            )
        })();
        let status = controller.status()?;
        let inactive = !status.cache.attached && !status.store_locked;
        drop(file);
        let cleanup = if inactive {
            (|| -> Result<()> {
                if r.container_root.join("owner.json").exists() {
                    OwnedVhd::reopen(&r.container_root)?.remove()?;
                }
                std::fs::remove_dir(&r.container_root)?;
                std::fs::remove_file(&fixture)?;
                Ok(())
            })()
        } else {
            Err("Preserve source and cache: host session is still active".into())
        };
        let mut value = match &test {
            Ok(v) => v.clone(),
            Err(e) => json!({"passed":false,"error":e.to_string()}),
        };
        value["ownedFixturesRemoved"] = json!(cleanup.is_ok());
        value["scope"] = json!("Native Windows host kernel, source restricted to a new file-backed VHDX; user data volumes are not enrolled");
        if let Err(e) = &cleanup {
            value["passed"] = json!(false);
            value["cleanupError"] = json!(e.to_string());
        }
        File::create_new(report)?.write_all(&serde_json::to_vec_pretty(&value)?)?;
        test?;
        cleanup
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if args.as_slice() == ["--status"] {
        println!(
            "{}",
            serde_json::to_string(&superhdd_platform::raw_lab::Controller::open()?.status()?)?
        );
        return Ok(());
    }
    #[cfg(windows)]
    if let [command, root] = args.as_slice() {
        if command == "--remove-owned-cache" {
            return proof::remove_owned_cache(root.as_ref());
        }
    }
    #[cfg(windows)]
    if let [tool, request, report] = args.as_slice() {
        return proof::run(tool.as_ref(), request.as_ref(), report.as_ref());
    }
    Err("host_session_probe SERVICE.exe OWNED_REQUEST.json NEW_REPORT.json".into())
}
