//! Native host proof through the already-installed, signed Dokan runtime.
//! A NEW read-only fixture only. Never enrolls/reletters existing data volumes.
#[cfg(windows)]
mod proof {
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::{
        ffi::{c_char, c_void},
        fs::{File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::{Path, PathBuf},
        sync::Mutex,
        time::Instant,
    };
    use superhdd_cache_core::{FileCache, Statistics, BLOCK_BYTES};
    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    type ReadCallback =
        unsafe extern "system" fn(*mut c_void, u64, *mut c_void, u32, *mut u32) -> i32;
    type Mount = unsafe extern "system" fn(ReadCallback, *mut c_void, u64) -> i32;
    type MountDirectory =
        unsafe extern "system" fn(ReadCallback, *mut c_void, u64, *const u16) -> i32;
    type Unmount = unsafe extern "system" fn() -> i32;
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(file: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> i32;
        fn CreateFileMappingW(
            file: *mut c_void,
            security: *mut c_void,
            protection: u32,
            high: u32,
            low: u32,
            name: *const u16,
        ) -> *mut c_void;
        fn MapViewOfFile(
            mapping: *mut c_void,
            access: u32,
            high: u32,
            low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const LENGTH: usize = 16 * 1024 * 1024;
    const BUDGET: u64 = 64 * 1024 * 1024;
    #[repr(align(4096))]
    struct Block([u8; BLOCK_BYTES]);
    fn fill(block: &mut Block, index: usize) {
        for (i, b) in block.0.iter_mut().enumerate() {
            *b = ((i * 17 + i / 11 + index * 31) % 251) as u8;
        }
    }
    struct State {
        cache: Mutex<FileCache>,
    }
    unsafe extern "system" fn read(
        context: *mut c_void,
        offset: u64,
        buffer: *mut c_void,
        length: u32,
        done: *mut u32,
    ) -> i32 {
        if done.is_null() || context.is_null() || (length != 0 && buffer.is_null()) {
            return 87;
        }
        // SAFETY: The bridge lends these buffers only for this callback. State is
        // pinned in a Box until DokanCloseHandle has joined all callback threads.
        unsafe {
            *done = 0;
        }
        if length == 0 {
            return 0;
        }
        std::panic::catch_unwind(|| {
            let state = unsafe { &*context.cast::<State>() };
            let output = unsafe { std::slice::from_raw_parts_mut(buffer.cast(), length as usize) };
            match state
                .cache
                .lock()
                .map_err(|_| ())
                .and_then(|mut c| c.read_at(offset, output).map_err(|_| ()))
            {
                Ok(count) => {
                    unsafe {
                        *done = count as u32;
                    }
                    0
                }
                Err(_) => 30,
            }
        })
        .unwrap_or(30)
    }
    struct Bridge {
        library: *mut c_void,
        unmount: Unmount,
    }
    impl Bridge {
        fn mount(path: &Path, state: &mut State, directory: Option<&Path>) -> Result<Self> {
            let wide: Vec<u16> = path
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let library =
                unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x100 | 0x800) };
            if library.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let start = unsafe {
                GetProcAddress(
                    library,
                    if directory.is_some() {
                        c"ShddMountDirectory".as_ptr()
                    } else {
                        c"ShddMount".as_ptr()
                    },
                )
            };
            let stop = unsafe { GetProcAddress(library, c"ShddUnmount".as_ptr()) };
            if start.is_null() || stop.is_null() {
                unsafe {
                    FreeLibrary(library);
                }
                return Err("Native ABI bridge exports are missing".into());
            }
            let unmount: Unmount = unsafe { std::mem::transmute(stop) };
            let owner = Self { library, unmount };
            let code = if let Some(point) = directory {
                let wide_point: Vec<u16> = point
                    .as_os_str()
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let mount: MountDirectory = unsafe { std::mem::transmute(start) };
                unsafe {
                    mount(
                        read,
                        std::ptr::from_mut(state).cast(),
                        LENGTH as u64,
                        wide_point.as_ptr(),
                    )
                }
            } else {
                let mount: Mount = unsafe { std::mem::transmute(start) };
                unsafe { mount(read, std::ptr::from_mut(state).cast(), LENGTH as u64) }
            };
            if code != 0 {
                return Err(format!("Owned read-only Dokan mount failed: {code}").into());
            }
            Ok(owner)
        }
    }
    impl Drop for Bridge {
        fn drop(&mut self) {
            // The native function closes only our private Dokan instance and
            // waits for its callbacks. It never issues a global driver stop.
            unsafe {
                (self.unmount)();
                FreeLibrary(self.library);
            }
        }
    }
    fn stats(state: &State) -> Result<Statistics> {
        Ok(state
            .cache
            .lock()
            .map_err(|_| "Cache lock failed")?
            .statistics()
            .clone())
    }
    fn direct(path: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(0x20000000)
            .open(path)
    }
    fn trace(file: &mut File) -> Result<f64> {
        let mut actual = Box::new(Block([0; BLOCK_BYTES]));
        let mut expected = Box::new(Block([0; BLOCK_BYTES]));
        let start = Instant::now();
        for i in 0..96 {
            let index = (i * 73 + 19) % (LENGTH / BLOCK_BYTES);
            file.seek(SeekFrom::Start((index * BLOCK_BYTES) as u64))?;
            file.read_exact(&mut actual.0)?;
            fill(&mut expected, index);
            if actual.0 != expected.0 {
                return Err("Incorrect file-system read bytes".into());
            }
        }
        Ok(start.elapsed().as_secs_f64())
    }
    fn mapped(path: &Path) -> Result<()> {
        let file = File::open(path)?;
        let map = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null_mut(),
                2,
                0,
                0,
                std::ptr::null(),
            )
        };
        if map.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let view = unsafe { MapViewOfFile(map, 4, 0, 0, LENGTH) };
        if view.is_null() {
            unsafe {
                CloseHandle(map);
            }
            return Err(std::io::Error::last_os_error().into());
        }
        let mut expected = Box::new(Block([0; BLOCK_BYTES]));
        let data = unsafe { std::slice::from_raw_parts(view.cast::<u8>(), LENGTH) };
        let mut correct = true;
        for i in 0..96 {
            let index = (i * 73 + 19) % (LENGTH / BLOCK_BYTES);
            fill(&mut expected, index);
            if data[index * BLOCK_BYTES..(index + 1) * BLOCK_BYTES] != expected.0 {
                correct = false;
                break;
            }
        }
        unsafe {
            UnmapViewOfFile(view);
            CloseHandle(map);
        }
        if !correct {
            return Err("Incorrect mapped file bytes".into());
        }
        Ok(())
    }
    pub fn run(run_root: &Path, bridge: &Path, directory_mount: bool) -> Result<()> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let runtime = workspace.join("artifacts/runtime").canonicalize()?;
        let run = run_root
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("dokan-local-"))
            .ok_or("Not an owned Dokan proof root")?;
        if run.len() != 32
            || !run.bytes().all(|b| b.is_ascii_hexdigit())
            || run_root
                .parent()
                .ok_or("No runtime parent")?
                .canonicalize()?
                != runtime
            || bridge.canonicalize()?
                != workspace
                    .join("artifacts/dokan-probe/bridge.dll")
                    .canonicalize()?
            || !run_root.is_dir()
            || std::fs::read_dir(run_root)?.next().is_some()
        {
            return Err("Require the new, empty run-owned directory and built bridge".into());
        }
        let directory_parent = PathBuf::from(format!(r"D:\SuperHDD-DokanDirectory-{run}"));
        let directory_point = directory_parent.join("visible");
        let source = if directory_mount {
            std::fs::create_dir(&directory_parent)?;
            std::fs::create_dir(&directory_point)?;
            directory_parent.join("backing.bin")
        } else {
            PathBuf::from(format!(r"D:\SuperHDD-Dokan-{run}.bin"))
        };
        let virtual_path = if directory_mount {
            directory_point.join("fixture.bin")
        } else {
            PathBuf::from(r"S:\fixture.bin")
        };
        let backing = run_root.join("ssd-cache.bin");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(0x10000000)
            .open(&source)?;
        let mut block = Box::new(Block([0; BLOCK_BYTES]));
        let mut expected_hash = Sha256::new();
        for index in 0..LENGTH / BLOCK_BYTES {
            fill(&mut block, index);
            file.write_all(&block.0)?;
            expected_hash.update(&block.0);
        }
        file.sync_all()?;
        drop(file);
        let mut baseline_file = direct(&source)?;
        let baseline = trace(&mut baseline_file)?;
        drop(baseline_file);
        let mut state = Box::new(State {
            cache: Mutex::new(FileCache::create_read_only_unbuffered(
                &source, &backing, BUDGET,
            )?),
        });
        let test = (|| -> Result<serde_json::Value> {
            if OpenOptions::new().write(true).open(&source).is_ok() {
                return Err("External writer was not excluded".into());
            }
            let mount = Bridge::mount(
                bridge,
                &mut state,
                directory_mount.then_some(directory_point.as_path()),
            )?;
            let mut virtual_file = direct(&virtual_path)?;
            let before = stats(&state)?;
            let cold_seconds = trace(&mut virtual_file)?;
            let cold = stats(&state)?;
            let warm_seconds = trace(&mut virtual_file)?;
            let warm = stats(&state)?;
            if warm.hits <= cold.hits
                || warm.ssd_read_bytes <= cold.ssd_read_bytes
                || warm.source_read_bytes != cold.source_read_bytes
            {
                return Err("Warm Windows reads did not use SSD exclusively".into());
            }
            let before_mapped = stats(&state)?;
            mapped(&virtual_path)?;
            let after_mapped = stats(&state)?;
            if after_mapped.hits <= before_mapped.hits {
                return Err("No SSD hits on mapped reads".into());
            }
            let mut readers = Vec::new();
            for _ in 0..4 {
                let reader_path = virtual_path.clone();
                readers.push(std::thread::spawn(
                    move || -> std::result::Result<(), String> {
                        trace(&mut direct(&reader_path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                        Ok(())
                    },
                ));
            }
            for reader in readers {
                reader.join().map_err(|_| "Reader thread failed")??;
            }
            if OpenOptions::new().write(true).open(&virtual_path).is_ok() {
                return Err("Test filesystem allowed writes".into());
            }
            state.cache.lock().map_err(|_| "Cache lock")?.pause(true);
            let paused = stats(&state)?;
            trace(&mut virtual_file)?;
            let bypass = stats(&state)?;
            if bypass.ssd_read_bytes != paused.ssd_read_bytes
                || bypass.source_read_bytes <= paused.source_read_bytes
            {
                return Err("Pause did not bypass SSD".into());
            }
            {
                let mut cache = state.cache.lock().map_err(|_| "Cache lock")?;
                cache.clear();
                cache.pause(false);
            }
            let cleared = stats(&state)?;
            trace(&mut virtual_file)?;
            let refill = stats(&state)?;
            trace(&mut virtual_file)?;
            let resumed = stats(&state)?;
            if refill.source_read_bytes <= cleared.source_read_bytes
                || resumed.hits <= refill.hits
                || resumed.source_read_bytes != refill.source_read_bytes
                || resumed.backing_errors != 0
                || resumed.checksum_failures != 0
            {
                return Err("Clear/resume or backing integrity check failed".into());
            }
            drop(virtual_file);
            drop(mount);
            if virtual_path.exists() {
                return Err("Owned mount was not removed".into());
            }
            Ok(
                json!({"passed":true,"scope":"Native Windows host, read-only NEW file via existing signed Dokan; existing D/E paths are not intercepted", "directoryMount":directory_mount,"virtualPath":virtual_path,"checks":{"nativeUnbufferedReadBytes":true,"actualSsdHits":true,"warmSourceApiAvoidance":true,"mappedReadBytesAndSsdHits":true,"concurrentReaders":true,"virtualWritesRejected":true,"sourceWriterExcluded":true,"pauseBypass":true,"clearResume":true,"ownedInstanceClosed":true},"timingSeconds":{"hddBaseline":baseline,"coldVirtual":cold_seconds,"warmVirtual":warm_seconds},"before":before,"cold":cold,"warm":warm,"mapped":after_mapped,"final":resumed,"dedicatedRamCacheBytes":0,"sourceBytes":LENGTH,"ssdBudgetBytes":BUDGET}),
            )
        })();
        drop(state); // Releases read-only source lease only after instance closes.
        let mut value = match &test {
            Ok(v) => v.clone(),
            Err(e) => json!({"passed":false,"error":e.to_string()}),
        };
        let cleanup = (|| -> Result<()> {
            if virtual_path.exists() {
                return Err("Preserve files while a virtual instance remains".into());
            }
            let mut source_file = File::open(&source)?;
            let mut actual = Sha256::new();
            loop {
                let count = source_file.read(&mut block.0)?;
                if count == 0 {
                    break;
                }
                actual.update(&block.0[..count]);
            }
            drop(source_file);
            if actual.finalize() != expected_hash.finalize() {
                return Err("Source changed; preserve fixtures".into());
            }
            std::fs::remove_file(&source)?;
            std::fs::remove_file(&backing)?;
            if directory_mount {
                // Nonrecursive removal: never follow a reparse point or remove contents.
                std::fs::remove_dir(&directory_point)?;
                std::fs::remove_dir(&directory_parent)?;
            }
            Ok(())
        })();
        value["ownedFilesRemoved"] = json!(cleanup.is_ok());
        if let Err(e) = &cleanup {
            value["passed"] = json!(false);
            value["cleanupError"] = json!(e.to_string());
        }
        File::create_new(run_root.join("result.json"))?
            .write_all(&serde_json::to_vec_pretty(&value)?)?;
        test?;
        cleanup
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if let [run, bridge] = args.as_slice() {
        return proof::run(run.as_ref(), bridge.as_ref(), false);
    }
    #[cfg(windows)]
    if let [run, bridge, flag] = args.as_slice() {
        if flag == "--directory-mount" {
            return proof::run(run.as_ref(), bridge.as_ref(), true);
        }
    }
    Err("dokan_cache_probe NEW_OWNED_RUN_ROOT BRIDGE.dll [--directory-mount]".into())
}
