//! Disposable file-only local iSCSI proof. No physical-device enrollment API.
#![cfg(windows)]
use cache_core::FileCache;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

const SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const CAPACITY: u64 = 32 * 1024 * 1024;
const MAX_IO: usize = 1024 * 1024;
mod device;
struct Probe {
    cache: FileCache,
    _cache_pins: Option<superhdd_storage_control::windows::protected::ProtectedDir>,
    root: PathBuf,
    writes: u64,
    recovered: bool,
    device_source: bool,
    watched: Vec<(u64, u64)>,
    watched_hits: u64,
    watched_source_bytes: u64,
    watched_ssd_bytes: u64,
}
impl Probe {
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> io::Result<usize> {
        let watched = self
            .watched
            .iter()
            .any(|&(start, end)| offset < end && offset + output.len() as u64 > start);
        let before = self.cache.statistics().clone();
        let result = self.cache.read_at(offset, output);
        if watched {
            let after = self.cache.statistics();
            self.watched_hits += after.hits - before.hits;
            self.watched_source_bytes += after.source_read_bytes - before.source_read_bytes;
            self.watched_ssd_bytes += after.ssd_read_bytes - before.ssd_read_bytes;
        }
        result
    }
}
static PROBE: OnceLock<Mutex<Option<Probe>>> = OnceLock::new();
fn state() -> &'static Mutex<Option<Probe>> {
    PROBE.get_or_init(|| Mutex::new(None))
}
fn ffi(action: impl FnOnce() -> io::Result<()>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            eprintln!("cache provider: {error}");
            -1
        }
        Err(_) => -2,
    }
}
fn invalid() -> io::Error {
    io::Error::other("Invalid disposable probe request")
}
fn with_probe(action: impl FnOnce(&mut Probe) -> io::Result<()>) -> io::Result<()> {
    let mut state = state().lock().map_err(|_| invalid())?;
    action(state.as_mut().ok_or_else(invalid)?)
}
fn range(offset: u64, length: usize) -> io::Result<()> {
    if length == 0
        || length > MAX_IO
        || !length.is_multiple_of(512)
        || !offset.is_multiple_of(512)
        || offset
            .checked_add(length as u64)
            .is_none_or(|end| end > SOURCE_BYTES)
    {
        return Err(invalid());
    }
    Ok(())
}

/// Caller supplies exactly 32 ASCII hex bytes; no paths or existing files accepted.
///
/// # Safety
/// `run` must point to `length` readable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_open(run: *const u8, length: u32) -> i32 {
    ffi(|| unsafe { open(run, length, false) })
}

/// Reopens only this probe's previously created, recorded disposable image.
/// Cache metadata is never reused across a process lifetime.
///
/// # Safety
/// `run` must point to `length` readable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_reopen(run: *const u8, length: u32) -> i32 {
    ffi(|| unsafe { open(run, length, true) })
}

/// Opens only the exact new VHDX recorded for this run, already offline.
/// # Safety
/// `run` must point to `length` readable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_open_device(run: *const u8, length: u32, number: u32) -> i32 {
    ffi(|| unsafe { open_device(run, length, number, false) })
}
/// Reuses only this run's owned cache file, with empty maps and an offline VHDX.
/// # Safety
/// `run` must point to `length` readable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_reopen_device(
    run: *const u8,
    length: u32,
    number: u32,
) -> i32 {
    ffi(|| unsafe { open_device(run, length, number, true) })
}
unsafe fn open_device(run: *const u8, length: u32, number: u32, recovered: bool) -> io::Result<()> {
    if run.is_null() || length != 32 {
        return Err(invalid());
    }
    let run = std::str::from_utf8(unsafe { std::slice::from_raw_parts(run, 32) })
        .map_err(|_| invalid())?;
    if !run.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut state = state().lock().map_err(|_| invalid())?;
    if state.is_some() {
        return Err(invalid());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/runtime")
        .join(format!("iscsi-local-{run}"));
    let (cache, pins) = device::open(run, &root, number, recovered)?;
    *state = Some(Probe {
        cache,
        _cache_pins: pins,
        root,
        writes: 0,
        recovered,
        device_source: true,
        watched: Vec::new(),
        watched_hits: 0,
        watched_source_bytes: 0,
        watched_ssd_bytes: 0,
    });
    Ok(())
}

unsafe fn open(run: *const u8, length: u32, recovered: bool) -> io::Result<()> {
    if run.is_null() || length != 32 {
        return Err(invalid());
    }
    let run = std::str::from_utf8(unsafe { std::slice::from_raw_parts(run, 32) })
        .map_err(|_| invalid())?;
    if !run.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut state = state().lock().map_err(|_| invalid())?;
    if state.is_some() {
        return Err(invalid());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/runtime")
        .join(format!("iscsi-local-{run}"));
    fs::create_dir_all(&root)?;
    let source = PathBuf::from(format!("D:\\SuperHDD-iSCSI-{run}.img"));
    let ownership = root.join("source-owner.json");
    let backing = root.join(if recovered {
        "ssd-cache-recovered.bin"
    } else {
        "ssd-cache.bin"
    });
    if recovered {
        use std::os::windows::fs::MetadataExt;
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&ownership)?)?;
        let metadata = fs::symlink_metadata(&source)?;
        // This is a disposable-file guard, not a production authorization record.
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || metadata.len() != SOURCE_BYTES
            || saved["runId"] != run
            || saved["sourceBytes"] != SOURCE_BYTES
            || saved["created"] != metadata.creation_time()
        {
            return Err(invalid());
        }
    } else {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&source)?;
        let zero = [0u8; 64 * 1024];
        for _ in 0..SOURCE_BYTES / zero.len() as u64 {
            file.write_all(&zero)?;
        }
        file.sync_all()?;
        use std::os::windows::fs::MetadataExt;
        let marker = serde_json::json!({"runId":run,"sourceBytes":SOURCE_BYTES,"created":file.metadata()?.creation_time()});
        let mut owner = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&ownership)?;
        owner.write_all(&serde_json::to_vec(&marker)?)?;
        owner.sync_all()?;
        drop(file);
    }
    let cache = FileCache::create_exclusive_unbuffered(&source, &backing, CAPACITY)?;
    *state = Some(Probe {
        cache,
        _cache_pins: None,
        root,
        writes: 0,
        recovered,
        device_source: false,
        watched: Vec::new(),
        watched_hits: 0,
        watched_source_bytes: 0,
        watched_ssd_bytes: 0,
    });
    Ok(())
}

/// Caller owns a writable buffer of `length` bytes for the entire synchronous call.
///
/// # Safety
/// `buffer` must point to `length` exclusively writable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_read(offset: u64, buffer: *mut u8, length: u32) -> i32 {
    ffi(|| {
        range(offset, length as usize)?;
        if buffer.is_null() {
            return Err(invalid());
        }
        with_probe(|probe| {
            let output = unsafe { std::slice::from_raw_parts_mut(buffer, length as usize) };
            if probe.read_at(offset, output)? != output.len() {
                return Err(invalid());
            }
            Ok(())
        })
    })
}

/// Caller owns a readable buffer of `length` bytes for the entire synchronous call.
///
/// # Safety
/// `buffer` must point to `length` readable bytes for the entire call.
#[no_mangle]
pub unsafe extern "C" fn cache_probe_write(offset: u64, buffer: *const u8, length: u32) -> i32 {
    ffi(|| {
        range(offset, length as usize)?;
        if buffer.is_null() {
            return Err(invalid());
        }
        with_probe(|probe| {
            let input = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
            if offset.is_multiple_of(4096) && input.len().is_multiple_of(4096) {
                probe.cache.write_source(offset, input)?;
            } else {
                // A 512-byte NTFS/GPT write must preserve neighboring source
                // sectors. The source lease and this mutex exclude all writers.
                let mut completed = 0;
                while completed < input.len() {
                    let position = offset + completed as u64;
                    let aligned = position / 4096 * 4096;
                    let within = (position % 4096) as usize;
                    let count = (4096 - within).min(input.len() - completed);
                    let mut sector = [0u8; 4096];
                    if probe.read_at(aligned, &mut sector)? != sector.len() {
                        return Err(invalid());
                    }
                    sector[within..within + count]
                        .copy_from_slice(&input[completed..completed + count]);
                    probe.cache.write_source(aligned, &sector)?;
                    completed += count;
                }
            }
            probe.writes += 1;
            Ok(())
        })
    })
}

#[no_mangle]
pub extern "C" fn cache_probe_control(operation: u32) -> i32 {
    ffi(|| {
        with_probe(|probe| {
            match operation {
                0 => {}
                1 => probe.cache.clear(),
                2 => probe.cache.pause(true),
                3 => probe.cache.pause(false),
                4 => {
                    probe.watched.clear();
                    probe.watched_hits = 0;
                    probe.watched_source_bytes = 0;
                    probe.watched_ssd_bytes = 0;
                }
                _ => return Err(invalid()),
            }
            let value = serde_json::json!({"statistics":probe.cache.statistics(),"writes":probe.writes,
            "sourceBytes":SOURCE_BYTES,"capacityBytes":CAPACITY,"occupancyBytes":probe.cache.occupancy(),
            "sourceExclusive":!probe.device_source,"sourceFilesystemOffline":probe.device_source,"windowsFileDataCacheBypassed":true,"everyWriteFlushedToSource":true,
            "recovered":probe.recovered,"cacheMappingsReused":false,
            "ssdPayloadProtected":probe._cache_pins.is_some(),"ssdFileExclusive":probe._cache_pins.is_some(),
            "watchedRanges":probe.watched,
            "watchedStatistics":{"hits":probe.watched_hits,"sourceReadBytes":probe.watched_source_bytes,"ssdReadBytes":probe.watched_ssd_bytes},
            "watchedScope":"Cache traffic caused by block requests overlapping registered fixture extents"});
            fs::write(
                probe.root.join("stats.tmp"),
                serde_json::to_vec_pretty(&value)?,
            )?;
            fs::rename(probe.root.join("stats.tmp"), probe.root.join("stats.json"))?;
            Ok(())
        })
    })
}

/// Register a fixture extent for measurement only; never changes I/O behavior.
#[no_mangle]
pub extern "C" fn cache_probe_watch(offset: u64, length: u64) -> i32 {
    ffi(|| {
        let end = offset
            .checked_add(length)
            .filter(|&end| end <= SOURCE_BYTES)
            .ok_or_else(invalid)?;
        if length == 0 || !offset.is_multiple_of(512) || !length.is_multiple_of(512) {
            return Err(invalid());
        }
        with_probe(|probe| {
            if probe.watched.len() >= 128
                || probe.watched.iter().any(|&(a, b)| offset < b && end > a)
            {
                return Err(invalid());
            }
            probe.watched.push((offset, end));
            Ok(())
        })
    })
}

/// Only call after the initiator has disconnected and all target callbacks ended.
#[no_mangle]
pub extern "C" fn cache_probe_close() -> i32 {
    ffi(|| {
        *state().lock().map_err(|_| invalid())? = None;
        Ok(())
    })
}
