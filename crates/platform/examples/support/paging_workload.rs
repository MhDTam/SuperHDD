//! Disposable-VM workload. Unique files avoid counting an earlier buffered
//! read as SSD acceleration; every result includes observed storage counters.
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    time::Instant,
};
use superhdd_platform::{
    paging_cache::DirectCache,
    paging_driver::{Controller, Status},
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{Memory::*, SystemInformation::*},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MIB: usize = 1024 * 1024;
#[repr(align(4096))]
struct Block([u8; 65536]);
fn expected(p: usize) -> u8 {
    ((p.wrapping_mul(17) ^ (p >> 9) ^ (p >> 16)) & 255) as u8
}
fn unbuffered(path: &Path, create: bool) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(create)
        .create_new(create)
        .share_mode(3)
        .custom_flags(0x20000000 | if create { 0x80000000 } else { 0 })
        .open(path)?)
}
struct Fixtures(Vec<PathBuf>);
impl Fixtures {
    fn seed(&mut self, path: PathBuf) -> Result<PathBuf> {
        let mut f = unbuffered(&path, true)?;
        self.0.push(path.clone());
        let mut b = Block([0; 65536]);
        for n in 0..256 {
            for (i, v) in b.0.iter_mut().enumerate() {
                *v = expected(n * 65536 + i);
            }
            f.write_all(&b.0)?;
        }
        f.sync_all()?;
        Ok(path)
    }
    fn remove(&mut self) -> Result<()> {
        while let Some(p) = self.0.last() {
            std::fs::remove_file(p)?;
            self.0.pop();
        }
        Ok(())
    }
}
impl Drop for Fixtures {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}
fn reads(file: &mut File) -> Result<()> {
    let mut b = Block([0; 65536]);
    for n in 0..256 {
        let index = (n * 7919) % 256;
        file.seek(SeekFrom::Start((index * 65536) as u64))?;
        file.read_exact(&mut b.0)?;
        for (i, v) in b.0.iter().enumerate() {
            if *v != expected(index * 65536 + i) {
                return Err("Workload byte oracle failed".into());
            }
        }
    }
    Ok(())
}
fn mapped(path: &Path) -> Result<()> {
    let f = File::open(path)?;
    let map = unsafe {
        CreateFileMappingW(
            f.as_raw_handle(),
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
    let view = unsafe { MapViewOfFile(map, FILE_MAP_READ, 0, 0, 16 * MIB) };
    if view.Value.is_null() {
        unsafe { CloseHandle(map) };
        return Err(std::io::Error::last_os_error().into());
    }
    let good=(0..256).all(|n|{let index=(n*7919)%256;(0..65536).all(|i|unsafe{*((view.Value as *const u8).add(index*65536+i))}==expected(index*65536+i))});
    unsafe {
        UnmapViewOfFile(view);
        CloseHandle(map)
    };
    if good {
        Ok(())
    } else {
        Err("Mapped workload byte oracle failed".into())
    }
}
fn memory() -> Result<MEMORYSTATUSEX> {
    let mut s = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(s)
    }
}
struct Pressure {
    regions: Vec<*mut std::ffi::c_void>,
    bytes: usize,
    before: u64,
    after: u64,
}
impl Pressure {
    fn acquire() -> Result<Self> {
        let first = memory()?;
        if first.ullTotalPhys > 3 * 1024 * 1024 * 1024 {
            return Err("Memory pressure is restricted to the <=3 GiB disposable guest".into());
        }
        let mut p = Self {
            regions: Vec::new(),
            bytes: 0,
            before: first.ullAvailPhys,
            after: first.ullAvailPhys,
        };
        while p.bytes < 1536 * MIB {
            let current = memory()?;
            if current.ullAvailPhys < 80 * MIB as u64 || current.ullAvailPageFile < 128 * MIB as u64
            {
                break;
            }
            let ptr = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    64 * MIB,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if ptr.is_null() {
                break;
            }
            p.regions.push(ptr);
            p.bytes += 64 * MIB;
            for offset in (0..64 * MIB).step_by(4096) {
                unsafe {
                    (ptr as *mut u8).add(offset).write_volatile(0x5a);
                }
            }
        }
        p.after = memory()?.ullAvailPhys;
        Ok(p)
    }
    fn report(&self) -> Value {
        json!({"allocatedBytes":self.bytes,"availablePhysicalBefore":self.before,"availablePhysicalDuring":self.after,"below128MiB":self.after<128*MIB as u64})
    }
}
impl Drop for Pressure {
    fn drop(&mut self) {
        for ptr in self.regions.drain(..) {
            unsafe { VirtualFree(ptr, 0, MEM_RELEASE) };
        }
    }
}
fn delta(before: &Status, after: &Status, elapsed: u128) -> Value {
    json!({"elapsedMicros":elapsed,"pagingHits":after.paging_hits-before.paging_hits,
    "ssdReadBytes":after.cache.ssd_read_bytes-before.cache.ssd_read_bytes,"sourceWorkerReadBytes":after.cache.source_read_bytes-before.cache.source_read_bytes,
    "pagingReadBypasses":after.paging_bypasses-before.paging_bypasses,"readBypasses":after.cache.read_bypasses-before.cache.read_bypasses,
    "errors":after.cache.errors-before.cache.errors})
}
fn measure(path: &Path, is_mapped: bool, driver: &Controller) -> Result<Value> {
    let before = driver.status()?;
    let started = Instant::now();
    if is_mapped {
        mapped(path)?;
    } else {
        reads(
            &mut OpenOptions::new()
                .read(true)
                .custom_flags(0x10000000)
                .open(path)?,
        )?;
    }
    let elapsed = started.elapsed().as_micros();
    let after = driver.status()?;
    Ok(delta(&before, &after, elapsed))
}
pub fn run(driver: &Controller, session: &mut DirectCache, run: &str) -> Result<Value> {
    if run.len() != 32 || !run.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Exact disposable run ID required".into());
    }
    let mut fixtures = Fixtures(Vec::new());
    let mut rounds = Vec::new();
    let outcome = (|| -> Result<Value> {
        for n in 0..6 {
            let a = fixtures.seed(PathBuf::from(format!(
                "S:\\shdd-paging-{run}-{n}-active.bin"
            )))?;
            let b = fixtures.seed(PathBuf::from(format!(
                "S:\\shdd-paging-{run}-{n}-paused.bin"
            )))?;
            let pressure = Pressure::acquire()?;
            session.pause(false)?;
            // Warm the SSD using direct I/O. The first ordinary/mapped read of
            // each unique file is measured exactly once, while pressure is held.
            reads(&mut unbuffered(&b, false)?)?;
            reads(&mut unbuffered(&a, false)?)?;
            let is_mapped = n >= 3;
            let paused;
            let active;
            if n % 2 == 0 {
                session.pause(true)?;
                paused = measure(&b, is_mapped, driver)?;
                session.pause(false)?;
                active = measure(&a, is_mapped, driver)?;
            } else {
                active = measure(&a, is_mapped, driver)?;
                session.pause(true)?;
                paused = measure(&b, is_mapped, driver)?;
                session.pause(false)?;
            }
            rounds.push(json!({"kind":if is_mapped{"mapped"}else{"buffered"},"first":if n%2==0{"paused"}else{"active"},"pressure":pressure.report(),"active":active,"paused":paused}));
            drop(pressure);
            fixtures.remove()?;
        }
        let observed = |kind: &str| {
            rounds.iter().filter(|r| r["kind"] == kind).any(|r| {
                r["active"]["pagingHits"].as_u64().unwrap_or(0) > 0
                    && r["active"]["ssdReadBytes"].as_u64().unwrap_or(0) > 0
            })
        };
        let buffered = observed("buffered");
        let mapped = observed("mapped");
        let no_errors = rounds
            .iter()
            .all(|r| r["active"]["errors"] == 0 && r["paused"]["errors"] == 0);
        Ok(
            json!({"rounds":rounds,"bufferedPagingSsdObserved":buffered,"mappedPagingSsdObserved":mapped,"byteOraclesPassed":true,"noNewCacheErrors":no_errors,
            "allTimingPairsObserveStorage":rounds.iter().all(|r|r["active"]["ssdReadBytes"].as_u64().unwrap_or(0)>0&&r["paused"]["pagingReadBypasses"].as_u64().unwrap_or(0)>0),
            "speedupClaimed":false,"passed":buffered&&mapped&&no_errors}),
        )
    })();
    let resume = session.pause(false);
    let cleanup = fixtures.remove();
    let report = outcome?;
    resume?;
    cleanup?;
    Ok(report)
}
