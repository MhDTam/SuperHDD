//! Exercises the actual service stdin controller and owned VHDX against the
//! signed v4 VM driver. This proves virtual lifecycle/data, not physical media.
#[cfg(windows)]
mod proof {
    use serde::Deserialize;
    use serde_json::{json, Value};
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        fs::{File, OpenOptions},
        io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
        os::windows::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Command, Stdio},
        sync::mpsc::{self, Receiver},
        time::{Duration, Instant},
    };
    use superhdd_platform::raw_lab::{Controller, Status};
    use superhdd_storage_control::{
        lab::LabIdentity,
        windows::{volume_identity, OwnedVhd, VerifiedLab},
    };
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        source_volume: String,
        container_root: PathBuf,
        budget_bytes: Option<u64>,
        acknowledged_disposable_source: bool,
        isolated_lab: LabIdentity,
    }
    struct Service {
        child: Child,
        input: Option<ChildStdin>,
        replies: Receiver<std::result::Result<Value, String>>,
        error_log: PathBuf,
    }
    impl Service {
        fn start(tool: &Path, request: &Path, log: &Path) -> Result<Self> {
            let mut child = Command::new(tool)
                .args(["--lab-cache-session"])
                .arg(request)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(File::create_new(log)?)
                .spawn()?;
            let input = child.stdin.take();
            let output = child.stdout.take().ok_or("Missing service output")?;
            let (sender, replies) = mpsc::channel();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    let mut line = Vec::new();
                    let result = Read::by_ref(&mut reader)
                        .take(4097)
                        .read_until(b'\n', &mut line);
                    match result {
                        Ok(0) => break,
                        Ok(_) if line.len() <= 4096 => {
                            if sender
                                .send(serde_json::from_slice(&line).map_err(|e| e.to_string()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        _ => {
                            let _ = sender.send(Err("Invalid or oversized service reply".into()));
                            break;
                        }
                    }
                }
            });
            Ok(Self {
                child,
                input,
                replies,
                error_log: log.to_path_buf(),
            })
        }
        fn reply(&self, phase: &str) -> Result<Value> {
            let reply = self
                .replies
                .recv_timeout(Duration::from_secs(45))
                .map_err(|error| {
                    let mut detail = String::new();
                    if let Ok(file) = File::open(&self.error_log) {
                        let _ = file.take(8192).read_to_string(&mut detail);
                    }
                    format!("Service reply failed: {error}; stderr: {detail}")
                })??;
            if reply["ok"] != true || reply["phase"] != phase {
                return Err(format!("Unexpected {phase} reply: {reply}").into());
            }
            Ok(reply)
        }
        fn command(&mut self, command: &str, phase: &str) -> Result<Value> {
            let input = self.input.as_mut().ok_or("Service input already closed")?;
            writeln!(input, "{command}")?;
            input.flush()?;
            self.reply(phase)
        }
        fn wait(&mut self) -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.try_wait()? {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(format!("Service exited {status}").into())
                    };
                }
                if Instant::now() >= deadline {
                    return Err("Service failed to exit after stop/EOF".into());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
    impl Drop for Service {
        fn drop(&mut self) {
            self.input.take();
            if self.wait().is_err() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
    struct Aligned(*mut u8);
    impl Aligned {
        fn new() -> Self {
            let layout = Layout::from_size_align(65536, 4096).unwrap();
            let p = unsafe { alloc_zeroed(layout) };
            if p.is_null() {
                std::alloc::handle_alloc_error(layout)
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
    fn pattern(buffer: &mut Aligned, block: usize, revision: usize) {
        for (i, b) in buffer.bytes().iter_mut().enumerate() {
            *b = ((i * 31 + i / 13 + block * 19 + revision * 71) % 251) as u8;
        }
    }
    fn reads(file: &mut File, revision: usize) -> Result<()> {
        let mut expected = Aligned::new();
        let mut actual = Aligned::new();
        file.seek(SeekFrom::Start(0))?;
        for block in 0..4 {
            pattern(&mut expected, block, if block == 0 { revision } else { 0 });
            file.read_exact(actual.bytes())?;
            if actual.bytes() != expected.bytes() {
                return Err(
                    format!("Source byte mismatch in block {block}, revision {revision}").into(),
                );
            }
        }
        Ok(())
    }
    fn owned_status(controller: &Controller, target: &Value) -> Result<Status> {
        let s = controller.status()?;
        if !s.cache.attached
            || s.cache.cache_disabled
            || !s.store_locked
            || s.pnp_state != 1
            || Some(u64::from(s.source_disk_number)) != target["sourceDisk"].as_u64()
            || Some(u64::from(s.backing_disk_number)) != target["cacheDisk"].as_u64()
        {
            return Err(
                "Kernel did not confirm the service-owned source/backing identities".into(),
            );
        }
        Ok(s)
    }
    fn same_target(s: &Status, target: &Value) -> bool {
        s.cache.attached
            && Some(u64::from(s.source_disk_number)) == target["sourceDisk"].as_u64()
            && Some(u64::from(s.backing_disk_number)) == target["cacheDisk"].as_u64()
    }
    fn detached(controller: &Controller) -> Result<()> {
        let s = controller.status()?;
        if s.cache.attached || s.store_locked {
            return Err("Kernel retained backing after service stop".into());
        }
        Ok(())
    }
    pub fn run(tool: &Path, request_file: &Path, report: &Path) -> Result<()> {
        let request: Request = serde_json::from_reader(File::open(request_file)?)?;
        let lab = VerifiedLab::verify(request.isolated_lab.clone())?;
        lab.validate_root(&request.container_root)?;
        if !request.acknowledged_disposable_source
            || request.source_volume != "I:"
            || request.container_root
                != Path::new(&format!("K:\\managed-{}", request.isolated_lab.run_id))
            || request.budget_bytes != Some(256 * 1024 * 1024)
            || request_file.parent() != report.parent()
        {
            return Err("Require the exact new run-owned VM request and report".into());
        }
        let source = volume_identity(&request.source_volume)?;
        if source.bus_type != 15 {
            return Err("Source must be the disposable child VHDX".into());
        }
        lab.validate_parent(&volume_identity("K:")?)?;
        let dependencies = superhdd_storage_control::windows::disk_dependencies(&format!(
            "\\\\.\\PhysicalDrive{}",
            source.disk_number
        ))?;
        File::create_new(
            report
                .parent()
                .ok_or("Missing report parent")?
                .join("preflight.json"),
        )?
        .write_all(&serde_json::to_vec_pretty(
            &json!({"source":source,"dependencies":dependencies}),
        )?)?;
        let controller = Controller::open()?;
        detached(&controller)?;
        std::fs::create_dir(&request.container_root)?;
        let fixture = PathBuf::from(format!(
            "I:\\managed-fixture-{}.bin",
            request.isolated_lab.run_id
        ));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(0x20000000 | 0x10000000)
            .open(&fixture)?;
        let mut data = Aligned::new();
        for block in 0..4 {
            pattern(&mut data, block, 0);
            file.write_all(data.bytes())?;
        }
        file.sync_all()?;
        let mut target = Value::Null;
        let test = (|| -> Result<Value> {
            let parent = report.parent().ok_or("Missing report parent")?;
            let mut first = Service::start(tool, request_file, &parent.join("service-create.log"))?;
            let started = first.reply("active")?;
            target = started["target"].clone();
            let before = owned_status(&controller, &target)?;
            reads(&mut file, 0)?;
            reads(&mut file, 0)?;
            reads(&mut file, 0)?;
            let warm = owned_status(&controller, &target)?;
            if warm.cache.hits <= before.cache.hits
                || warm.cache.ssd_read_bytes <= before.cache.ssd_read_bytes
            {
                return Err("No real cache hit after native managed-session creation".into());
            }
            first.command("pause", "paused")?;
            let paused = owned_status(&controller, &target)?;
            if !paused.cache.paused {
                return Err("Kernel did not pause".into());
            }
            reads(&mut file, 0)?;
            let paused_after = owned_status(&controller, &target)?;
            if paused_after.cache.ssd_read_bytes != paused.cache.ssd_read_bytes {
                return Err("Paused reads used SSD cache".into());
            }
            first.command("clear", "paused")?;
            let cleared = owned_status(&controller, &target)?;
            if cleared.cache.generation <= paused_after.cache.generation {
                return Err("Clear did not invalidate the old cache generation".into());
            }
            first.command("resume", "active")?;
            let before_refill = owned_status(&controller, &target)?;
            reads(&mut file, 0)?;
            let refilled = owned_status(&controller, &target)?;
            // FillBytes is cumulative traffic, not current occupancy. A clear
            // must force this previously warm file back through source/fill.
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
                return Err(format!(
                    "Cleared fixture was not refilled from source: {before_refill:?}; {refilled:?}"
                )
                .into());
            }
            reads(&mut file, 0)?;
            file.seek(SeekFrom::Start(0))?;
            pattern(&mut data, 0, 1);
            file.write_all(data.bytes())?;
            file.sync_all()?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            let written = owned_status(&controller, &target)?;
            first.command("stop", "stopped")?;
            first.wait()?;
            drop(first);
            detached(&controller)?;
            let original = OwnedVhd::reopen_in_lab(&request.container_root, lab.clone())?;
            let instance = original.instance();
            let vhd_path = original.path().to_path_buf();
            drop(original);
            let reopen = parent.join("reopen-request.json");
            let mut value: Value = serde_json::from_reader(File::open(request_file)?)?;
            value["budgetBytes"] = Value::Null;
            File::create_new(&reopen)?.write_all(&serde_json::to_vec_pretty(&value)?)?;
            let mut second = Service::start(tool, &reopen, &parent.join("service-reopen.log"))?;
            let restarted = second.reply("active")?;
            target = restarted["target"].clone();
            let imported = owned_status(&controller, &target)?;
            if imported.imported_records == 0 {
                return Err("Reopen did not import persisted RAW cache records".into());
            }
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            let revalidated = owned_status(&controller, &target)?;
            if revalidated.revalidated_records == 0
                || revalidated.cache.ssd_read_bytes <= imported.cache.ssd_read_bytes
            {
                return Err(
                    "Reopened records were not byte-revalidated and used for SSD reads".into(),
                );
            }
            second.input.take();
            second.reply("stopped")?;
            second.wait()?;
            drop(second);
            detached(&controller)?;
            let mut third = Service::start(tool, &reopen, &parent.join("service-crash.log"))?;
            target = third.reply("active")?["target"].clone();
            owned_status(&controller, &target)?;
            reads(&mut file, 1)?;
            third.child.kill()?;
            third.child.wait()?;
            third.input.take();
            drop(third);
            // Deliberate operator reconciliation, not automatic crash recovery.
            let crashed = controller.status()?;
            // A killed owner may already have caused bypass/detachment. Like
            // Session::stop, reconciliation permits disabling an exact owned
            // bypassed session, never resuming it or changing a foreign one.
            if crashed.cache.attached {
                if !same_target(&crashed, &target) {
                    return Err("Foreign session after service kill; preserved".into());
                }
                controller.disable()?;
            }
            detached(&controller)?;
            let mut fourth = Service::start(tool, &reopen, &parent.join("service-reconciled.log"))?;
            target = fourth.reply("active")?["target"].clone();
            owned_status(&controller, &target)?;
            reads(&mut file, 1)?;
            reads(&mut file, 1)?;
            fourth.command("stop", "stopped")?;
            fourth.wait()?;
            drop(fourth);
            detached(&controller)?;
            let owned = OwnedVhd::reopen_in_lab(&request.container_root, lab.clone())?;
            if owned.instance() != instance || owned.path() != vhd_path {
                return Err("Recovery replaced the owned VHDX".into());
            }
            drop(owned);
            Ok(
                json!({"passed":true,"checks":{"createdByActualService":true,"exactKernelIdentities":true,
                "coldAndWarmBytes":true,"ssdHits":true,"pausedReadsBypassCache":true,"clearAndResume":true,
                "sourceWriteInvalidation":true,"stopBeforeBackingDetach":true,"sameOwnedContainerReopened":true,
                "persistedRecordsRevalidated":true,"eofStops":true,"killedServiceOperatorReconciled":true},
                "before":before,"warm":warm,"paused":paused,"cleared":cleared,"refilledAfterClear":refilled,"afterWrite":written,
                "imported":imported,"revalidated":revalidated,"afterServiceKill":crashed}),
            )
        })();
        // Only reconcile a known exact target. An unknown session is preserved.
        let cleanup = (|| -> Result<()> {
            let s = controller.status()?;
            if s.cache.attached {
                if !same_target(&s, &target) {
                    return Err("Unknown cache ownership during cleanup; preserved".into());
                }
                controller.disable()?;
            }
            detached(&controller)?;
            drop(file);
            if request.container_root.exists() {
                if request.container_root.join("owner.json").exists() {
                    OwnedVhd::reopen_in_lab(&request.container_root, lab)?.remove()?;
                }
                std::fs::remove_dir(&request.container_root)?;
            }
            std::fs::remove_file(&fixture)?;
            Ok(())
        })();
        let mut value = match &test {
            Ok(v) => v.clone(),
            Err(e) => json!({"passed":false,"error":e.to_string()}),
        };
        value["scope"]=json!("Actual native managed service lifecycle on separately owned virtual parents in the isolated signed-driver VM. No physical-media performance or production qualification.");
        value["automaticCrashRecoveryProven"] = json!(false);
        value["ownedFixturesRemoved"] = json!(cleanup.is_ok());
        if let Err(e) = &cleanup {
            value["passed"] = json!(false);
            value["cleanupError"] = json!(e.to_string());
        }
        File::create_new(report)?.write_all(&serde_json::to_vec_pretty(&value)?)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        test?;
        cleanup
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("managed_session_probe SERVICE.exe REQUEST.json NEW_REPORT.json\nProvisioned isolated Super HDD VM only.");
        return Ok(());
    }
    #[cfg(windows)]
    if let [tool, request, report] = args.as_slice() {
        return proof::run(tool.as_ref(), request.as_ref(), report.as_ref());
    }
    Err("Windows VM and three explicit arguments required".into())
}
