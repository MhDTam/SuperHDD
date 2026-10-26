//! Fixed-trace ordinary-file benchmark for the disposable signed-driver VM.
//! Never uses FileCache; no disk, partition, installation or policy operations.
#[cfg(windows)]
mod lab {
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        alloc::{alloc_zeroed, dealloc, Layout},
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
            process::CommandExt,
        },
        path::{Path, PathBuf},
        time::Instant,
    };
    use superhdd_platform::raw_lab::{Attachment, Controller};
    const BLOCK: usize = 65536;
    const BLOCKS: usize = 8192;
    type Error = Box<dyn std::error::Error>;
    struct Aligned(*mut u8);
    impl Aligned {
        fn new() -> Self {
            let p = unsafe { alloc_zeroed(Layout::from_size_align(BLOCK, 4096).unwrap()) };
            if p.is_null() {
                std::alloc::handle_alloc_error(Layout::from_size_align(BLOCK, 4096).unwrap());
            }
            Self(p)
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
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Manifest {
        version: u32,
        root: PathBuf,
        source_disk: u32,
        blocks: usize,
        block_bytes: usize,
        written_sha256: String,
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
    fn require_vm() -> Result<(), Error> {
        let status=std::process::Command::new("powershell.exe").creation_flags(0x08000000).args(["-NoProfile","-NonInteractive","-Command","$c=Get-CimInstance Win32_ComputerSystem;if($c.Manufacturer -eq 'Microsoft Corporation' -and $c.Model -eq 'Virtual Machine' -and $env:COMPUTERNAME -eq 'SHDD-LAB'){exit 0}else{exit 9}"]).status()?;
        if !status.success() {
            return Err("Benchmark refused: disposable Super HDD VM only".into());
        }
        Ok(())
    }
    fn root_letter(root: &Path) -> Result<char, Error> {
        let text = root.to_str().ok_or("Invalid benchmark path")?;
        let b = text.as_bytes();
        if b.len() < 35
            || !b[0].is_ascii_alphabetic()
            || &b[1..3] != b":\\"
            || !text[3..].starts_with("superhdd-kernel-bench-")
            || text[3..].contains(['\\', '/', ':', '.'])
        {
            return Err("Use a new root-level X:\\superhdd-kernel-bench-UNIQUE directory".into());
        }
        Ok(b[0] as char)
    }
    fn disk(letter: char) -> Result<u32, Error> {
        let volume = File::open(format!("\\\\.\\{letter}:"))?;
        let mut extents = Extents::default();
        let mut returned = 0;
        let ok = unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
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
        if ok == 0
            || returned as usize != std::mem::size_of::<Extents>()
            || extents.count != 1
            || extents.extent.start < 0
            || extents.extent.length <= 0
        {
            return Err("Invalid benchmark source extent".into());
        }
        Ok(extents.extent.disk)
    }
    fn direct(path: &Path, create: bool) -> Result<File, Error> {
        Ok(OpenOptions::new()
            .read(true)
            .write(create)
            .create_new(create)
            // Noncached fixture writes are flushed once in prepare(). Per-write
            // FUA would add thousands of unrelated platter flushes to setup.
            .custom_flags(0x20000000 | 0x10000000)
            .open(path)?)
    }
    fn pattern(block: usize, bytes: &mut [u8]) {
        for (word, out) in bytes.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            out.copy_from_slice(
                &(0x45ec_927b_a81d_13f0u64
                    ^ ((block as u64) << 32)
                    ^ (word as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .to_le_bytes(),
            );
        }
    }
    fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(&serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }
    fn load_manifest(path: &Path) -> Result<Manifest, Error> {
        let bytes = std::fs::read(path)?;
        if bytes.len() > 8192 {
            return Err("Oversized benchmark manifest".into());
        }
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        if manifest.version != 1
            || ![1024, BLOCKS].contains(&manifest.blocks)
            || manifest.block_bytes != BLOCK
            || disk(root_letter(&manifest.root)?)? != manifest.source_disk
        {
            return Err("Benchmark manifest/source identity mismatch".into());
        }
        let dir = std::fs::symlink_metadata(&manifest.root)?;
        let file = std::fs::symlink_metadata(manifest.root.join("owned-fixture.bin"))?;
        if !dir.is_dir()
            || dir.file_attributes() & 0x400 != 0
            || !file.is_file()
            || file.file_attributes() & 0x4e00 != 0
            || file.len() != (BLOCK * manifest.blocks) as u64
        {
            return Err("Benchmark fixture changed or redirected".into());
        }
        Ok(manifest)
    }
    fn prepare(
        request: &Path,
        root: &Path,
        manifest_path: &Path,
        blocks: usize,
    ) -> Result<(), Error> {
        if ![1024, BLOCKS].contains(&blocks) {
            return Err("Use 1024 diagnostic blocks or 8192 benchmark blocks".into());
        }
        let source_disk = disk(root_letter(root)?)?;
        let bytes = std::fs::read(request)?;
        if bytes.len() > 8192 {
            return Err("Oversized request".into());
        }
        let request: Attachment = serde_json::from_slice(&bytes)?;
        let driver = Controller::open()?;
        if driver.status()?.cache.attached {
            return Err("Disable the previous lab session before preparing a benchmark".into());
        }
        driver.configure(&request)?;
        if driver.status()?.source_disk_number != source_disk {
            return Err("Benchmark path differs from the configured source".into());
        }
        driver.pause(true)?;
        std::fs::create_dir(root)?;
        let mut memory = Aligned::new();
        let path = root.join("owned-fixture.bin");
        let mut file = direct(&path, true)?;
        let mut digest = Sha256::new();
        for block in 0..blocks {
            pattern(block, memory.bytes());
            file.write_all(memory.bytes())?;
            digest.update(memory.bytes());
        }
        file.sync_all()?;
        drop(file);
        save(
            manifest_path,
            &Manifest {
                version: 1,
                root: root.to_owned(),
                source_disk,
                blocks,
                block_bytes: BLOCK,
                written_sha256: format!("{:x}", digest.finalize()),
            },
        )?;
        println!("{{\"prepared\":true,\"fixtureBytes\":{}}}", BLOCK * blocks);
        Ok(())
    }
    fn read_trace(manifest_path: &Path, mode: &str, report: &Path) -> Result<(), Error> {
        let manifest = load_manifest(manifest_path)?;
        let driver = Controller::open()?;
        let initial = driver.status()?;
        if !initial.cache.attached
            || initial.cache.cache_disabled
            || initial.source_disk_number != manifest.source_disk
            || initial.pnp_state != 1
        {
            return Err("Benchmark driver session/source mismatch".into());
        }
        match mode {
            "baseline" => driver.pause(true)?,
            "cold" => {
                driver.pause(false)?;
                driver.clear()?;
            }
            "warm" => {
                if initial.cache.paused {
                    return Err("Cold phase must precede warm".into());
                }
            }
            _ => return Err("Unknown benchmark mode".into()),
        }
        let mut file = direct(&manifest.root.join("owned-fixture.bin"), false)?;
        let mut actual = Aligned::new();
        let mut expected = Aligned::new();
        let mut digest = Sha256::new();
        let mut latencies = Vec::with_capacity(manifest.blocks);
        let before = driver.status()?;
        let started = Instant::now();
        let result = (|| -> Result<(), Error> {
            for i in 0..manifest.blocks {
                let block = i * 73 % manifest.blocks;
                pattern(block, &mut expected.bytes()[..4096]);
                let begin = Instant::now();
                file.seek(SeekFrom::Start((block * BLOCK) as u64))?;
                file.read_exact(&mut actual.bytes()[..4096])?;
                latencies.push(begin.elapsed().as_nanos() as u64);
                if actual.bytes()[..4096] != expected.bytes()[..4096] {
                    return Err(format!("Incorrect benchmark data at block {block}").into());
                }
                digest.update(&actual.bytes()[..4096]);
            }
            Ok(())
        })();
        let seconds = started.elapsed().as_secs_f64();
        let after = driver.status()?;
        latencies.sort_unstable();
        let value = serde_json::json!({"passed":result.is_ok(),"error":result.as_ref().err().map(|e|e.to_string()),"mode":mode,"scope":"Ordinary noncached 4 KiB Win32 reads, QD1, fixed permutation over the recorded owned disposable source file through the signed VM kernel driver. Physical host counters are recorded separately.",
            "fixtureBytes":BLOCK*manifest.blocks,"readBytes":manifest.blocks*4096,"operations":latencies.len(),"seconds":seconds,"iops":latencies.len() as f64/seconds,
            "p50Milliseconds":latencies.get(latencies.len()/2).map(|n|*n as f64/1e6),"p95Milliseconds":latencies.get(latencies.len()*95/100).map(|n|*n as f64/1e6),
            "sha256":format!("{:x}",digest.finalize()),"before":before,"after":after,
            "sourceReadBytesDelta":after.cache.source_read_bytes.saturating_sub(before.cache.source_read_bytes),"ssdReadBytesDelta":after.cache.ssd_read_bytes.saturating_sub(before.cache.ssd_read_bytes),
            "hitsDelta":after.cache.hits.saturating_sub(before.cache.hits),"errorsDelta":after.cache.errors.saturating_sub(before.cache.errors),"diagnosticAfter":driver.diagnostic()?});
        save(report, &value)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        result
    }
    fn cleanup(path: &Path) -> Result<(), Error> {
        let manifest = load_manifest(path)?;
        let driver = Controller::open()?;
        let status = driver.status()?;
        if status.cache.attached && status.source_disk_number != manifest.source_disk {
            return Err("Refusing to disable another lab session".into());
        }
        driver.disable()?;
        let fixture = manifest.root.join("owned-fixture.bin");
        let mut file = direct(&fixture, false)?;
        let mut bytes = Aligned::new();
        let mut expected = Aligned::new();
        file.read_exact(&mut bytes.bytes()[..4096])?;
        pattern(0, &mut expected.bytes()[..4096]);
        if bytes.bytes()[..4096] != expected.bytes()[..4096] {
            return Err("Fixture ownership check failed; retained for inspection".into());
        }
        drop(file);
        std::fs::remove_file(fixture)?;
        std::fs::remove_dir(&manifest.root)?;
        println!("{{\"cleaned\":true}}");
        Ok(())
    }
    include!("raw_stress/mod.rs");
    pub fn run(args: &[String]) -> Result<(), Error> {
        require_vm()?;
        match args {
            [action, request, root, manifest] if action == "prepare" => prepare(
                Path::new(request),
                Path::new(root),
                Path::new(manifest),
                BLOCKS,
            ),
            [action, request, root, manifest, blocks] if action == "prepare" => prepare(
                Path::new(request),
                Path::new(root),
                Path::new(manifest),
                blocks.parse()?,
            ),
            [action, manifest, mode, report] if action == "read" => {
                read_trace(Path::new(manifest), mode, Path::new(report))
            }
            [action, manifest] if action == "cleanup" => cleanup(Path::new(manifest)),
            [action, manifest, request, report] if action == "stress" => {
                stress(Path::new(manifest), Path::new(request), Path::new(report))
            }
            _ => Err("Invalid benchmark arguments; use --help".into()),
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("raw_benchmark prepare REQUEST.json NEW_SOURCE_ROOT MANIFEST.json [BLOCKS=8192] | read MANIFEST.json baseline|cold|warm REPORT.json | stress MANIFEST.json REQUEST.json REPORT.json | cleanup MANIFEST.json\nOnly the disposable Super HDD VM. Creates its own 512 MiB fixture (1024 blocks selects a 64 MiB diagnostic); never changes disks, partitions, signing or OS policy.");
        return Ok(());
    }
    #[cfg(windows)]
    {
        lab::run(&args)
    }
    #[cfg(not(windows))]
    {
        Err("Windows only".into())
    }
}
