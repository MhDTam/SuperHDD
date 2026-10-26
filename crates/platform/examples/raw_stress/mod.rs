// Included in raw_benchmark's VM-only module. Each request owns its event,
// OVERLAPPED, aligned buffer and file reference until actual I/O completion.
struct AsyncRead {
    file: std::sync::Arc<File>,
    memory: Aligned,
    overlapped: Box<windows_sys::Win32::System::IO::OVERLAPPED>,
    issued: bool,
    block: usize,
    targeted: bool,
}
impl AsyncRead {
    fn new(file: std::sync::Arc<File>, block: usize) -> Result<Self, Error> {
        let event = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                1,
                0,
                std::ptr::null(),
            )
        };
        if event.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut overlapped = Box::<windows_sys::Win32::System::IO::OVERLAPPED>::default();
        overlapped.hEvent = event;
        let offset = (block * BLOCK) as u64;
        overlapped.Anonymous.Anonymous.Offset = offset as u32;
        overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
        Ok(Self {
            file,
            memory: Aligned::new(),
            overlapped,
            issued: false,
            block,
            targeted: false,
        })
    }
    fn issue(&mut self) -> Result<bool, Error> {
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::ReadFile(
                self.file.as_raw_handle(),
                self.memory.0,
                BLOCK as u32,
                std::ptr::null_mut(),
                &mut *self.overlapped,
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(997) {
                return Err(error.into());
            }
        }
        self.issued = true;
        Ok(ok == 0)
    }
    fn cancel(&mut self) -> Result<bool, Error> {
        self.targeted = true;
        let ok = unsafe {
            windows_sys::Win32::System::IO::CancelIoEx(self.file.as_raw_handle(), &*self.overlapped)
        };
        if ok == 0 && std::io::Error::last_os_error().raw_os_error() != Some(1168) {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(ok != 0)
    }
    fn finish(&mut self) -> Result<bool, Error> {
        self.finish_with_changing_prefix(0)
    }
    fn finish_with_changing_prefix(&mut self, prefix: usize) -> Result<bool, Error> {
        if unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                self.overlapped.hEvent,
                30_000,
            )
        } != 0
        {
            return Err("Async read exceeded the 30 second completion deadline; ownership retained during drain".into());
        }
        let mut bytes = 0;
        let ok = unsafe {
            windows_sys::Win32::System::IO::GetOverlappedResult(
                self.file.as_raw_handle(),
                &*self.overlapped,
                &mut bytes,
                0,
            )
        };
        let error = std::io::Error::last_os_error();
        // A signaled per-request event is terminal. Never release a pending IRP.
        if ok == 0 && error.raw_os_error() == Some(996) {
            return Err(error.into());
        }
        self.issued = false;
        if ok == 0 {
            if error.raw_os_error() == Some(995) && self.targeted {
                return Ok(false);
            }
            return Err(error.into());
        }
        let mut expected = Aligned::new();
        pattern(self.block, expected.bytes());
        if bytes as usize != BLOCK || self.memory.bytes()[prefix..] != expected.bytes()[prefix..] {
            return Err(format!("Concurrent read data mismatch at block {}", self.block).into());
        }
        Ok(true)
    }
}
impl Drop for AsyncRead {
    fn drop(&mut self) {
        if self.issued {
            // CancelIoEx only requests cancellation. Even error paths must wait
            // for completion before the kernel stops owning these allocations.
            unsafe {
                windows_sys::Win32::System::IO::CancelIoEx(
                    self.file.as_raw_handle(),
                    &*self.overlapped,
                );
                let mut bytes = 0;
                windows_sys::Win32::System::IO::GetOverlappedResult(
                    self.file.as_raw_handle(),
                    &*self.overlapped,
                    &mut bytes,
                    1,
                );
            }
        }
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.overlapped.hEvent);
        }
    }
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct StressCounts {
    operations: usize,
    pending_at_submission: usize,
    cancel_attempts: usize,
    cancel_requests_accepted: usize,
    cancelled_completions: usize,
    correct_completions: usize,
    maximum_sampled_driver_pending: u32,
    controls_with_reads_pending: usize,
}
fn stress_source_writes(
    manifest: &Manifest,
    driver: &Controller,
) -> Result<serde_json::Value, Error> {
    let path = manifest.root.join("owned-fixture.bin");
    let asynchronous = std::sync::Arc::new(
        OpenOptions::new()
            .read(true)
            .custom_flags(0x20000000 | 0x10000000 | 0x40000000)
            .open(&path)?,
    );
    let mut writer = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(0x20000000 | 0x10000000)
        .open(&path)?;
    let mut reader = direct(&path, false)?;
    let mut actual = Aligned::new();
    let mut expected = Aligned::new();
    let mut patch = Aligned::new();
    let offset = (17 * BLOCK) as u64;
    let before = driver.status()?;
    let mut writes_with_pending = 0;
    let mut correct_tail_reads = 0;
    let mut correct_full_reads_after_write = 0;
    let result = (|| -> Result<(), Error> {
        for round in 0..32 {
            let mut reads = Vec::with_capacity(32);
            for _ in 0..32 {
                reads.push(AsyncRead::new(asynchronous.clone(), 17)?);
            }
            for read in &mut reads {
                read.issue()?;
            }
            writes_with_pending += usize::from(driver.status()?.cache.pending_reads > 0);
            pattern(17, expected.bytes());
            pattern(10_000 + round, patch.bytes());
            expected.bytes()[..4096].copy_from_slice(&patch.bytes()[..4096]);
            writer.seek(SeekFrom::Start(offset))?;
            writer.write_all(&patch.bytes()[..4096])?;
            writer.sync_all()?;
            for read in &mut reads {
                // These reads overlap the write in time: only the immutable
                // tail has a single valid oracle. Full-block freshness is
                // checked below after the write has completed and reads drain.
                if !read.finish_with_changing_prefix(4096)? {
                    return Err("Unrequested write-stress cancellation".into());
                }
                correct_tail_reads += 1;
            }
            for _ in 0..2 {
                reader.seek(SeekFrom::Start(offset))?;
                reader.read_exact(actual.bytes())?;
                if actual.bytes() != expected.bytes() {
                    return Err("Stale data after independent source write completion".into());
                }
                correct_full_reads_after_write += 1;
            }
        }
        if writes_with_pending == 0 || driver.status()?.cache.pending_reads != 0 {
            return Err("Concurrent source-write coverage/drain criterion failed".into());
        }
        Ok(())
    })();
    // Restore the owned fixture even when a data check fails. Block zero,
    // used by cleanup's ownership check, is never modified by this test.
    pattern(17, patch.bytes());
    writer.seek(SeekFrom::Start(offset))?;
    writer.write_all(&patch.bytes()[..4096])?;
    writer.sync_all()?;
    result?;
    Ok(
        serde_json::json!({"mode":"concurrent-source-writes","passed":true,
        "overlappingReads":correct_tail_reads,"immutableBytesCheckedPerOverlappingRead":BLOCK-4096,
        "fullBlockChecksAfterCompletedWrite":correct_full_reads_after_write,"writes":32,
        "writesWithDriverReadsPending":writes_with_pending,"fixtureRestored":true,
        "before":before,"after":driver.status()?}),
    )
}
fn stress(manifest_path: &Path, request_path: &Path, report: &Path) -> Result<(), Error> {
    let manifest = load_manifest(manifest_path)?;
    let bytes = std::fs::read(request_path)?;
    if bytes.len() > 8192 {
        return Err("Oversized stress configuration".into());
    }
    let request: Attachment = serde_json::from_slice(&bytes)?;
    let driver = Controller::open()?;
    let initial = driver.status()?;
    if !initial.cache.attached
        || initial.source_disk_number != manifest.source_disk
        || initial.pnp_state != 1
    {
        return Err("Stress session/source mismatch".into());
    }
    driver.pause(false)?;
    let file = std::sync::Arc::new(
        OpenOptions::new()
            .read(true)
            .custom_flags(0x20000000 | 0x10000000 | 0x40000000)
            .open(manifest.root.join("owned-fixture.bin"))?,
    );
    let mut phases = Vec::new();
    let started = Instant::now();
    let result = (|| -> Result<(), Error> {
        for mode in [
            "concurrent",
            "cancel-half",
            "clear-during-reads",
            "pause-during-reads",
            "disable-during-reads",
        ] {
            let mut counts = StressCounts::default();
            let before = driver.status()?;
            for round in 0..32 {
                // Clearing before each cancellation round keeps the test from
                // depending on a warm SSD completing before cancellation arrives.
                if mode == "cancel-half" {
                    driver.clear()?;
                }
                let mut reads = Vec::with_capacity(32);
                for index in 0..32 {
                    reads.push(AsyncRead::new(
                        file.clone(),
                        ((round * 32 + index) * 73) % manifest.blocks,
                    )?);
                }
                for read in &mut reads {
                    counts.pending_at_submission += usize::from(read.issue()?);
                    counts.operations += 1;
                }
                let pending = driver.status()?.cache.pending_reads;
                counts.maximum_sampled_driver_pending =
                    counts.maximum_sampled_driver_pending.max(pending);
                match mode {
                    "cancel-half" => {
                        for index in (1..32).step_by(2) {
                            counts.cancel_attempts += 1;
                            counts.cancel_requests_accepted += usize::from(reads[index].cancel()?);
                        }
                    }
                    "clear-during-reads" => {
                        counts.controls_with_reads_pending += usize::from(pending > 0);
                        driver.clear()?;
                    }
                    "pause-during-reads" => {
                        counts.controls_with_reads_pending += usize::from(pending > 0);
                        driver.pause(true)?;
                    }
                    "disable-during-reads" => {
                        counts.controls_with_reads_pending += usize::from(pending > 0);
                        driver.disable()?;
                    }
                    _ => {}
                }
                for read in &mut reads {
                    if read.finish()? {
                        counts.correct_completions += 1;
                    } else {
                        counts.cancelled_completions += 1;
                    }
                }
                if mode == "disable-during-reads" {
                    if driver.status()?.cache.attached {
                        return Err("Disable left a live cache session".into());
                    }
                    driver.configure(&request)?;
                    if driver.status()?.source_disk_number != manifest.source_disk {
                        return Err("Reconfigured source changed".into());
                    }
                }
                if mode == "pause-during-reads" {
                    driver.pause(false)?;
                }
                if driver.status()?.cache.pending_reads != 0 {
                    return Err("Driver reads did not drain".into());
                }
            }
            let after = driver.status()?;
            let cancellations = after.cancelled_reads.saturating_sub(before.cancelled_reads);
            let passed = counts.operations
                == counts.correct_completions + counts.cancelled_completions
                && counts.maximum_sampled_driver_pending <= 16
                && (mode != "concurrent" || counts.maximum_sampled_driver_pending > 1)
                && (mode != "cancel-half"
                    || (counts.cancelled_completions > 0 && cancellations > 0))
                && (!mode.ends_with("during-reads") || counts.controls_with_reads_pending > 0);
            phases.push(serde_json::json!({"mode":mode,"passed":passed,"counts":counts,
                "cancelledDriverReadsDelta":cancellations,
                "childCancellationsDelta":after.child_cancellations.saturating_sub(before.child_cancellations),
                "before":before,"after":after}));
            if !passed {
                return Err(format!("Stress coverage/completion criterion failed: {mode}").into());
            }
        }
        phases.push(stress_source_writes(&manifest, &driver)?);
        Ok(())
    })();
    let value = serde_json::json!({"passed":result.is_ok(),"error":result.as_ref().err().map(|e|e.to_string()),
        "scope":"Five phases of 32 batches of 32 ordinary noncached overlapped 64 KiB Win32 reads with full-block comparison; a sixth phase checks immutable tails of 1024 reads overlapping 32 source writes and performs 64 full-block freshness checks after write completion. Owned disposable VM fixture only. No injected lower-driver delays, low-memory, power or fault qualification.",
        "seconds":started.elapsed().as_secs_f64(),"phases":phases,"finalStatus":driver.status()?});
    save(report, &value)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    result
}
