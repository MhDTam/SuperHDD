//! Ordinary Win32 file-I/O proof for an already configured signed isolated-VM lab.
//! This executable never imports FileCache and never installs or loads a driver.
#[cfg(windows)]
mod windows {
    use sha2::{Digest, Sha256};
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle, process::CommandExt},
        path::{Path, PathBuf},
        time::Instant,
    };
    enum Controller {
        Pnp(superhdd_platform::pnp_lab::Controller),
        Raw(superhdd_platform::raw_lab::Controller),
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Status {
        cache: superhdd_platform::driver::Telemetry,
        source_disk_number: u32,
        pnp_state: u32,
        details: serde_json::Value,
    }
    impl Controller {
        fn configure_raw(
            &self,
            request: &superhdd_platform::raw_lab::Attachment,
        ) -> Result<(), String> {
            match self {
                Self::Raw(driver) => driver.configure(request),
                _ => Err("RAW-only operation".into()),
            }
        }
        fn open(raw: bool) -> Result<Self, String> {
            Ok(if raw {
                Self::Raw(superhdd_platform::raw_lab::Controller::open()?)
            } else {
                Self::Pnp(superhdd_platform::pnp_lab::Controller::open()?)
            })
        }
        fn status(&self) -> Result<Status, String> {
            match self {
                Self::Pnp(driver) => {
                    let s = driver.status()?;
                    Ok(Status {
                        cache: s.cache.clone(),
                        source_disk_number: s.source_disk_number,
                        pnp_state: s.pnp_state,
                        details: serde_json::to_value(s).map_err(|e| e.to_string())?,
                    })
                }
                Self::Raw(driver) => {
                    let s = driver.status()?;
                    Ok(Status {
                        cache: s.cache.clone(),
                        source_disk_number: s.source_disk_number,
                        pnp_state: s.pnp_state,
                        details: serde_json::to_value(s).map_err(|e| e.to_string())?,
                    })
                }
            }
        }
        fn pause(&self, paused: bool) -> Result<(), String> {
            match self {
                Self::Pnp(d) => d.pause(paused),
                Self::Raw(d) => d.pause(paused),
            }
        }
        fn clear(&self) -> Result<(), String> {
            match self {
                Self::Pnp(d) => d.clear(),
                Self::Raw(d) => d.clear(),
            }
        }
        fn disable(&self) -> Result<(), String> {
            match self {
                Self::Pnp(d) => d.disable(),
                Self::Raw(d) => d.disable(),
            }
        }
    }
    const BLOCK: usize = 65536;
    const BLOCKS: usize = 256;
    struct Aligned(*mut u8);
    impl Aligned {
        fn new() -> Self {
            let layout = Layout::from_size_align(BLOCK, 4096).unwrap();
            let pointer = unsafe { alloc_zeroed(layout) };
            if pointer.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            Self(pointer)
        }
        fn bytes(&mut self) -> &mut [u8] {
            unsafe { std::slice::from_raw_parts_mut(self.0, BLOCK) }
        }
    }
    impl Drop for Aligned {
        fn drop(&mut self) {
            unsafe { dealloc(self.0, Layout::from_size_align(BLOCK, 4096).unwrap()) }
        }
    }
    fn direct(path: &Path, write: bool, create: bool) -> std::io::Result<File> {
        OpenOptions::new()
            .read(true)
            .write(write)
            .create_new(create)
            .custom_flags(0x20000000 | if write { 0x80000000 } else { 0 })
            .open(path)
    }
    fn pattern(block: usize, bytes: &mut [u8], modified: bool) {
        for (word, output) in bytes.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            output.copy_from_slice(
                &(0xe527_a604_91f3_b82du64
                    ^ ((block as u64) << 32)
                    ^ (word as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .to_le_bytes(),
            );
        }
        if modified && block == 0 {
            bytes[4096..8192].fill(0xa5);
        }
    }
    fn verify(
        file: &mut File,
        modified: bool,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut actual = Aligned::new();
        let mut expected = Aligned::new();
        let mut digest = Sha256::new();
        let start = Instant::now();
        for i in 0..BLOCKS {
            let block = i * 73 % BLOCKS;
            pattern(block, expected.bytes(), modified);
            file.seek(SeekFrom::Start((block * BLOCK) as u64))?;
            file.read_exact(actual.bytes())?;
            if actual.bytes() != expected.bytes() {
                return Err(format!("Stale or corrupt bytes in fixture block {block}").into());
            }
            digest.update(actual.bytes());
        }
        Ok(
            serde_json::json!({"sha256":format!("{:x}",digest.finalize()),"bytes":BLOCK*BLOCKS,"seconds":start.elapsed().as_secs_f64()}),
        )
    }
    #[repr(C)]
    #[derive(Default)]
    struct Extent {
        disk: u32,
        start: i64,
        length: i64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Extents {
        count: u32,
        extent: Extent,
    }
    fn disk(letter: char) -> Result<u32, Box<dyn std::error::Error>> {
        let volume = File::open(format!("\\\\.\\{letter}:"))?;
        let mut output = Extents::default();
        let mut returned = 0;
        let ok = unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
                volume.as_raw_handle(),
                0x00560000,
                std::ptr::null(),
                0,
                (&mut output as *mut Extents).cast(),
                std::mem::size_of::<Extents>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if returned as usize != std::mem::size_of::<Extents>()
            || output.count != 1
            || output.extent.start < 0
            || output.extent.length <= 0
        {
            return Err("Test source must have one physical extent".into());
        }
        Ok(output.extent.disk)
    }
    fn require_vm() -> Result<(), Box<dyn std::error::Error>> {
        let output = std::process::Command::new("powershell.exe").creation_flags(0x08000000)
            .args(["-NoProfile","-NonInteractive","-Command","$c=Get-CimInstance Win32_ComputerSystem; if (($c.Manufacturer -eq 'Microsoft Corporation' -and $c.Model -eq 'Virtual Machine') -or $c.Manufacturer -match '^VMware' -or $c.Model -match '^(VirtualBox|KVM|Standard PC.*Q35|Standard PC.*i440FX)') { exit 0 } else { exit 9 }"]).output()?;
        if !output.status.success() {
            return Err("Probe refused: run only inside an isolated disposable Windows VM".into());
        }
        Ok(())
    }
    fn query_readonly(letter: char, code: u32) -> Result<u32, Box<dyn std::error::Error>> {
        let volume = File::open(format!("\\\\.\\{letter}:"))?;
        let mut returned = 0;
        let mut output = [0u8; 512];
        let ok = unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
                volume.as_raw_handle(),
                code,
                std::ptr::null(),
                0,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        // A non-clustered Server guest normally rejects this read-only query.
        // Failure must preserve the hot set just as success must.
        Ok(if ok == 0 {
            std::io::Error::last_os_error().raw_os_error().unwrap_or(-1) as u32
        } else {
            0
        })
    }
    fn mapped_verify(path: &Path, modified: bool) -> Result<(), Box<dyn std::error::Error>> {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Memory::*};
        let file = File::open(path)?;
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READONLY,
                0,
                0,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, BLOCK * BLOCKS) };
        if view.Value.is_null() {
            unsafe { CloseHandle(mapping) };
            return Err(std::io::Error::last_os_error().into());
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let bytes =
                unsafe { std::slice::from_raw_parts(view.Value.cast::<u8>(), BLOCK * BLOCKS) };
            let mut expected = Aligned::new();
            for block in 0..BLOCKS {
                pattern(block, expected.bytes(), modified);
                if bytes[block * BLOCK..(block + 1) * BLOCK] != *expected.bytes() {
                    return Err(format!("Mapped data mismatch in block {block}").into());
                }
            }
            Ok(())
        })();
        unsafe {
            UnmapViewOfFile(view);
            CloseHandle(mapping)
        };
        result
    }
    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }
    fn corrupt_owned_payload(
        request: &superhdd_platform::raw_lab::Attachment,
        disk_number: u32,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        use windows_sys::Win32::System::IO::DeviceIoControl;
        let mut volume = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0x20000000)
            .open(format!("\\\\?\\GLOBALROOT{}", request.cache_device))?;
        let mut extents = Extents::default();
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                volume.as_raw_handle(),
                0x00560000,
                std::ptr::null(),
                0,
                (&mut extents as *mut Extents).cast(),
                std::mem::size_of::<Extents>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || extents.count != 1 || extents.extent.disk != disk_number {
            return Err("Corruption test cache identity mismatch".into());
        }
        if unsafe {
            DeviceIoControl(
                volume.as_raw_handle(),
                0x90018,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut sector = Aligned::new();
        volume.read_exact(&mut sector.bytes()[..4096])?;
        let header = &sector.bytes()[..128];
        let mut checked = header.to_vec();
        checked[72..80].fill(0);
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let u64_at = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
        let slots = u32_at(24) as usize;
        let data_offset = u64_at(32);
        let total = u64_at(16);
        if &header[..8] != b"SHDDRAW4"
            || u32_at(8) != 4
            || u32_at(52) != 2
            || fnv(&checked) != u64_at(72)
            || total > request.capacity_bytes
            || data_offset + slots as u64 * 65536 != total
            || data_offset < 4096 + (slots as u64) * 64
        {
            return Err("Corruption test refused unknown cache header".into());
        }
        let mut targets = Vec::new();
        let mut index = Aligned::new();
        let mut page = u64::MAX;
        for slot in 0..slots {
            let offset = 4096 + slot as u64 * 64;
            let current = offset & !4095;
            if page != current {
                volume.seek(SeekFrom::Start(current))?;
                volume.read_exact(&mut index.bytes()[..4096])?;
                page = current;
            }
            let start = (offset - page) as usize;
            let record = &index.bytes()[start..start + 64];
            if u32::from_le_bytes(record[48..52].try_into().unwrap()) == 0x54494d43
                && fnv(&record[..56]) == u64::from_le_bytes(record[56..64].try_into().unwrap())
            {
                targets.push(data_offset + slot as u64 * 65536);
            }
        }
        if targets.is_empty() {
            return Err("No committed payload to corrupt".into());
        }
        for offset in &targets {
            volume.seek(SeekFrom::Start(*offset))?;
            volume.read_exact(&mut sector.bytes()[..4096])?;
            sector.bytes()[0] ^= 0x80;
            volume.seek(SeekFrom::Start(*offset))?;
            volume.write_all(&sector.bytes()[..4096])?;
        }
        volume.sync_all()?;
        Ok(targets.len())
    }
    fn raw_extras(
        driver: &Controller,
        request: &superhdd_platform::raw_lab::Attachment,
        path: &Path,
        file: &mut File,
        checkpoints: &mut Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let before = driver.status()?;
        mapped_verify(path, false)?;
        let mapped = driver.status()?;
        checkpoints.push(serde_json::json!({"phase":"mapped","status":mapped}));
        let paging_before = before.details["pagingHits"].as_u64().unwrap_or(0);
        let paging_after = mapped.details["pagingHits"].as_u64().unwrap_or(0);
        if paging_after <= paging_before {
            return Err("Mapped read did not demonstrate kernel paging cache hits".into());
        }
        driver.disable()?;
        driver.configure_raw(request)?;
        let reopened = driver.status()?;
        if reopened.details["importedRecords"].as_u64().unwrap_or(0) == 0 {
            return Err("No persistent index records imported".into());
        }
        verify(file, false)?;
        let revalidated = driver.status()?;
        checkpoints.push(serde_json::json!({"phase":"revalidated","status":revalidated}));
        if revalidated.details["revalidatedRecords"]
            .as_u64()
            .unwrap_or(0)
            == 0
        {
            return Err("No source revalidation after reopen".into());
        }
        verify(file, false)?;
        let reused = driver.status()?;
        if reused.cache.hits <= revalidated.cache.hits {
            return Err("No warm hits after source revalidation".into());
        }
        driver.disable()?;
        let mut writer = direct(path, true, false)?;
        writer.seek(SeekFrom::Start(4096))?;
        let mut changed = Aligned::new();
        changed.bytes()[..4096].fill(0xa5);
        writer.write_all(&changed.bytes()[..4096])?;
        writer.sync_all()?;
        drop(writer);
        driver.configure_raw(request)?;
        verify(file, true)?;
        verify(file, true)?;
        let offline_changed = driver.status()?;
        checkpoints.push(
            serde_json::json!({"phase":"sourceChangedWhileDisabled","status":offline_changed}),
        );
        let backing = offline_changed.details["backingDiskNumber"]
            .as_u64()
            .ok_or("Missing cache identity")? as u32;
        driver.disable()?;
        let corrupted_records = corrupt_owned_payload(request, backing)?;
        driver.configure_raw(request)?;
        verify(file, true)?;
        let prior = driver.status()?;
        verify(file, true)?;
        let recovered = driver.status()?;
        checkpoints.push(serde_json::json!({"phase":"corruptionRecovered","status":recovered}));
        if recovered.cache.errors <= prior.cache.errors
            || recovered.cache.source_read_bytes <= prior.cache.source_read_bytes
        {
            return Err("No detected-corruption/source-fallback proof".into());
        }
        verify(file, true)?;
        let healed = driver.status()?;
        checkpoints.push(serde_json::json!({"phase":"healed","status":healed}));
        if healed.cache.errors != recovered.cache.errors
            || healed.cache.hits <= recovered.cache.hits
        {
            return Err(format!("Payload repair warm check failed: errors {} -> {}, hits {} -> {}, generation {} -> {}, source bytes {} -> {}",recovered.cache.errors,healed.cache.errors,recovered.cache.hits,healed.cache.hits,recovered.cache.generation,healed.cache.generation,recovered.cache.source_read_bytes,healed.cache.source_read_bytes).into());
        }
        Ok(
            serde_json::json!({"passed":true,"pagingHitsAdded":paging_after-paging_before,"imported":reopened.details,"revalidated":revalidated.details,
            "offlineSourceChange":offline_changed.details,"corruptedPayloadRecords":corrupted_records,"corruptionRecovered":recovered.details,"healed":healed.details,
            "scope":"Actual mapped paging reads, session reopen, current-source validation, source write while disabled, and payload corruption fallback; no cold-boot or power-loss claim."}),
        )
    }
    pub fn run(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
        let mut normalized = args.to_vec();
        let raw = normalized.get(1).is_some_and(|arg| arg == "--raw");
        if raw {
            normalized.remove(1);
        }
        let args = normalized.as_slice();
        if args.len() != 3 && !(raw && args.len() == 4) {
            return Err("pnp_probe NEW_SOURCE_ROOT_DIRECTORY REPORT.json; existing configured PnP VM lab only".into());
        }
        require_vm()?;
        let driver = Controller::open(raw)?;
        let initial = driver.status()?;
        if !initial.cache.attached || initial.cache.cache_disabled || initial.pnp_state != 1 {
            return Err("Configure a fresh mounted NTFS lab source first".into());
        }
        let directory = PathBuf::from(&args[1]);
        let text = directory.to_str().ok_or("Invalid fixture directory")?;
        let bytes = text.as_bytes();
        if bytes.len() < 24
            || !bytes[0].is_ascii_alphabetic()
            || &bytes[1..3] != b":\\"
            || !text[3..].starts_with("superhdd-pnp-proof-")
            || text[3..].contains(['\\', '/', ':'])
        {
            return Err("Use a new root-level X:\\superhdd-pnp-proof-UNIQUE directory on the disposable source".into());
        }
        if disk(bytes[0] as char)? != initial.source_disk_number {
            return Err("Fixture path differs from the driver's source disk".into());
        }
        std::fs::create_dir(&directory)?;
        let source = directory.join("owned-fixture.bin");
        let mut created = false;
        let mut checkpoints = Vec::new();
        let result = (|| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
            let mut memory = Aligned::new();
            let mut file = direct(&source, true, true)?;
            created = true;
            for block in 0..BLOCKS {
                pattern(block, memory.bytes(), false);
                file.write_all(memory.bytes())?;
            }
            file.sync_all()?;
            drop(file);
            let mut file = direct(&source, false, false)?;
            driver.pause(true)?;
            let baseline = verify(&mut file, false)?;
            driver.pause(false)?;
            driver.clear()?;
            let before = driver.status()?;
            let cold = verify(&mut file, false)?;
            let populated = driver.status()?;
            let warm = verify(&mut file, false)?;
            let warmed = driver.status()?;
            checkpoints.push(serde_json::json!({"phase":"initialWarm","status":warmed,"baseline":baseline,"cold":cold,"warm":warm}));
            if populated.cache.fill_bytes <= before.cache.fill_bytes
                || warmed.cache.hits <= populated.cache.hits
                || warmed.cache.ssd_read_bytes <= populated.cache.ssd_read_bytes
            {
                return Err(
                    "No driver-backed SSD-hit proof: fill/hit counters did not increase".into(),
                );
            }
            let cluster_query_status = query_readonly(bytes[0] as char, 0x00560030)?;
            let name_query_status = query_readonly(bytes[0] as char, 0x004d0008)?;
            let identity_query_status = query_readonly(bytes[0] as char, 0x004d0000)?;
            verify(&mut file, false)?;
            let after_query = driver.status()?;
            if after_query.cache.ssd_read_bytes <= warmed.cache.ssd_read_bytes {
                return Err("Read-only cluster query invalidated the entire hot set".into());
            }
            checkpoints.push(serde_json::json!({"phase":"volumeQueriesRetainCache","clusterQueryStatus":cluster_query_status,"nameQueryStatus":name_query_status,"identityQueryStatus":identity_query_status,"status":after_query}));
            let extras = if raw && args.len() == 4 {
                let bytes = std::fs::read(&args[3])?;
                if bytes.len() > 8192 {
                    return Err("Oversized RAW request".into());
                }
                Some(raw_extras(
                    &driver,
                    &serde_json::from_slice(&bytes)?,
                    &source,
                    &mut file,
                    &mut checkpoints,
                )?)
            } else {
                None
            };
            let mut independent = direct(&source, true, false)?;
            memory.bytes().fill(0xa5);
            independent.seek(SeekFrom::Start(4096))?;
            independent.write_all(&memory.bytes()[..4096])?;
            independent.sync_all()?;
            drop(independent);
            // A small unrelated write must not discard the entire hot set.
            let unaffected_before = driver.status()?;
            let block = BLOCKS / 2;
            pattern(block, memory.bytes(), true);
            let mut retained = Aligned::new();
            file.seek(SeekFrom::Start((block * BLOCK) as u64))?;
            file.read_exact(retained.bytes())?;
            let unaffected_after = driver.status()?;
            if retained.bytes() != memory.bytes()
                || unaffected_after.cache.ssd_read_bytes <= unaffected_before.cache.ssd_read_bytes
            {
                return Err("An unrelated source write discarded a hot cached block".into());
            }
            let after_write = verify(&mut file, true)?;
            if driver.status()?.cache.generation <= warmed.cache.generation {
                return Err("Source write did not invalidate the driver epoch".into());
            }
            driver.pause(true)?;
            let paused_before = driver.status()?;
            let paused = verify(&mut file, true)?;
            let paused_after = driver.status()?;
            if paused_after.cache.hits != paused_before.cache.hits {
                return Err("Paused cache served hits".into());
            }
            driver.pause(false)?;
            driver.clear()?;
            let after_clear = verify(&mut file, true)?;
            drop(file);
            driver.disable()?;
            let mut file = direct(&source, false, false)?;
            let detached = verify(&mut file, true)?;
            Ok(
                serde_json::json!({"passed":true,"scope":"Ordinary noncached Win32 file I/O through the actual mounted-volume driver in an isolated VM. Internal driver counters are not physical-host-HDD-I/O proof.",
                "baseline":baseline,"cold":cold,"warm":warm,"afterWrite":after_write,"paused":paused,"afterClear":after_clear,"disabled":detached,
                "before":before,"populated":populated,"warmed":warmed,"rawExtras":extras,"checks":["byteCorrect","ssdFill","ssdHit","volumeQueriesRetainCache","independentWriteInvalidation","unrelatedWriteRetainsHotBlock","pauseBypass","clear","disablePassThrough"],
                "notCovered":["concurrent cancellation","PnP/power fault injection","physical host-disk ETW","cold-boot persistence","game loading"],"pagingAndReopenTested":extras.is_some()}),
            )
        })();
        let mut report = match result {
            Ok(value) => value,
            Err(error) => serde_json::json!({"passed":false,"error":error.to_string()}),
        };
        report["checkpoints"] = checkpoints.into();
        match driver.disable() {
            Ok(()) => {
                let cleanup = (|| -> std::io::Result<()> {
                    if created {
                        std::fs::remove_file(&source)?;
                    }
                    std::fs::remove_dir(&directory)
                })();
                report["cleanupSucceeded"] = cleanup.is_ok().into();
                if let Err(error) = cleanup {
                    report["passed"] = false.into();
                    report["cleanupError"] = error.to_string().into();
                }
            }
            Err(error) => {
                report["passed"] = false.into();
                report["cleanupSucceeded"] = false.into();
                report["disableError"] = error.into();
                report["retainedFixture"] = source.display().to_string().into();
            }
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?
            .write_all(&serde_json::to_vec_pretty(&report)?)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report["passed"] != true {
            return Err("Mounted-volume proof failed; inspect the report".into());
        }
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 2 && args[1] == "--help" {
        println!("pnp_probe [--raw] NEW_SOURCE_ROOT_DIRECTORY REPORT.json [RAW_REQUEST.json]\nRun only inside an isolated VM with an already configured signed v3 PnP or v4 RAW lab driver. Creates and removes its own 16 MiB fixture, tests ordinary Win32 I/O, writes independent source data, pauses/clears/disables the lab session. Optional RAW_REQUEST enables mapped reads, session reopen and intentional corruption of the owned disposable cache payload. No FileCache API, installation, disk creation, or OS policy change.");
        return Ok(());
    }
    #[cfg(windows)]
    {
        windows::run(&args)
    }
    #[cfg(not(windows))]
    {
        Err("Windows is required".into())
    }
}
