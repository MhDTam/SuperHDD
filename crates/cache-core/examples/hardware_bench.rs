//! Controlled file-level cache benchmark. Never a transparent volume-cache proof.
use sha2::{Digest, Sha256};
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::Instant,
};
use superhdd_cache_core::{FileCache, BLOCK_BYTES};

struct Aligned(*mut u8);
impl Aligned {
    fn new() -> Self {
        let layout = Layout::from_size_align(BLOCK_BYTES, 4096).unwrap();
        let p = unsafe { alloc_zeroed(layout) };
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Self(p)
    }
    fn bytes(&mut self, len: usize) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0, len) }
    }
}
impl Drop for Aligned {
    fn drop(&mut self) {
        unsafe { dealloc(self.0, Layout::from_size_align(BLOCK_BYTES, 4096).unwrap()) }
    }
}

#[cfg(windows)]
fn direct_file(path: &std::path::Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(0x20000000)
        .open(path)
}
#[cfg(not(windows))]
fn direct_file(_: &std::path::Path) -> std::io::Result<File> {
    Err(std::io::Error::other("Windows unbuffered I/O is required"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    name: String,
    round: usize,
    requests: usize,
    bytes: u64,
    seconds: f64,
    mib_per_second: f64,
    iops: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    sha256: String,
}
fn measure(
    name: &str,
    round: usize,
    offsets: &[u64],
    length: usize,
    mut read: impl FnMut(u64, &mut [u8]) -> std::io::Result<()>,
) -> std::io::Result<Measurement> {
    let mut memory = Aligned::new();
    let output = memory.bytes(length);
    let mut latencies = Vec::with_capacity(offsets.len());
    let mut digest = Sha256::new();
    let start = Instant::now();
    for &offset in offsets {
        let tick = Instant::now();
        read(offset, output)?;
        latencies.push(tick.elapsed().as_secs_f64() * 1000.0);
        digest.update(&*output);
    }
    let seconds = start.elapsed().as_secs_f64();
    latencies.sort_by(f64::total_cmp);
    let quantile = |p: f64| latencies[((latencies.len() - 1) as f64 * p).round() as usize];
    let bytes = (offsets.len() * length) as u64;
    let result = Measurement {
        name: name.into(),
        round,
        requests: offsets.len(),
        bytes,
        seconds,
        mib_per_second: bytes as f64 / 1048576.0 / seconds,
        iops: offsets.len() as f64 / seconds,
        p50_ms: quantile(0.50),
        p95_ms: quantile(0.95),
        p99_ms: quantile(0.99),
        sha256: format!("{:x}", digest.finalize()),
    };
    println!(
        "{} round {}: {:.2} MiB/s, {:.2} ms p95",
        name, round, result.mib_per_second, result.p95_ms
    );
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 {
        return Err(
            "Usage: hardware_bench NEW_SOURCE_DIRECTORY NEW_CACHE_DIRECTORY CAPACITY_BYTES".into(),
        );
    }
    let source_dir = PathBuf::from(&args[1]);
    let cache_dir = PathBuf::from(&args[2]);
    let capacity: u64 = args[3].to_str().ok_or("Invalid capacity")?.parse()?;
    if capacity > 25 * 1024u64.pow(3) {
        return Err("Maximum authorized benchmark capacity is 25 GiB".into());
    }
    fs::create_dir(&source_dir)?;
    fs::create_dir(&cache_dir)?;
    let source = source_dir.join("superhdd-disposable-source.bin");
    let backing = cache_dir.join("ssd-cache.bin");
    let fixture_bytes = 1024u64.pow(3);
    let mut fixture = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)?;
    let mut chunk = vec![0u8; 1024 * 1024];
    let mut random = 0xb74a973f56194e21u64;
    println!("Creating 1 GiB disposable source at {}", source.display());
    for _ in 0..1024 {
        for bytes in chunk.as_chunks_mut::<8>().0 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            bytes.copy_from_slice(&random.to_le_bytes());
        }
        fixture.write_all(&chunk)?;
    }
    fixture.sync_all()?;
    drop(fixture);
    let mut original = vec![0u8; BLOCK_BYTES];
    File::open(&source)?.read_exact(&mut original)?;
    let mut direct = direct_file(&source)?;
    #[cfg(windows)]
    let mut cache = FileCache::create_unbuffered(&source, &backing, capacity)?;
    #[cfg(not(windows))]
    let mut cache = FileCache::create(&source, &backing, capacity)?;
    let offsets: Vec<u64> = (0..1536)
        .map(|i| ((i * 7919 + 17) % 16384) as u64 * BLOCK_BYTES as u64)
        .collect();
    let mut measurements = Vec::new();
    let mut expected64 = String::new();
    for round in 1..=3 {
        let row = measure(
            "HDD direct random 64 KiB",
            round,
            &offsets,
            BLOCK_BYTES,
            |offset, buffer| {
                direct.seek(SeekFrom::Start(offset))?;
                direct.read_exact(buffer)
            },
        )?;
        if round == 1 {
            expected64 = row.sha256.clone();
        } else {
            assert_eq!(row.sha256, expected64);
        }
        measurements.push(row);
    }
    let row = measure(
        "Cache cold fill random 64 KiB",
        1,
        &offsets,
        BLOCK_BYTES,
        |offset, buffer| {
            assert_eq!(cache.read_at(offset, buffer)?, buffer.len());
            Ok(())
        },
    )?;
    assert_eq!(row.sha256, expected64);
    measurements.push(row);
    let source_before_warm = cache.statistics().source_read_bytes;
    for round in 1..=3 {
        let row = measure(
            "Cache warm random 64 KiB",
            round,
            &offsets,
            BLOCK_BYTES,
            |offset, buffer| {
                assert_eq!(cache.read_at(offset, buffer)?, buffer.len());
                Ok(())
            },
        )?;
        assert_eq!(row.sha256, expected64);
        measurements.push(row);
    }
    assert_eq!(cache.statistics().source_read_bytes, source_before_warm);
    for round in 1..=3 {
        let baseline = measure(
            "HDD direct random 4 KiB",
            round,
            &offsets,
            4096,
            |offset, buffer| {
                direct.seek(SeekFrom::Start(offset))?;
                direct.read_exact(buffer)
            },
        )?;
        let warm = measure(
            "Cache warm random 4 KiB",
            round,
            &offsets,
            4096,
            |offset, buffer| {
                assert_eq!(cache.read_at(offset, buffer)?, buffer.len());
                Ok(())
            },
        )?;
        assert_eq!(baseline.sha256, warm.sha256);
        measurements.extend([baseline, warm]);
    }
    cache.pause(true);
    let row = measure(
        "Cache paused random 64 KiB",
        1,
        &offsets,
        BLOCK_BYTES,
        |offset, buffer| {
            assert_eq!(cache.read_at(offset, buffer)?, buffer.len());
            Ok(())
        },
    )?;
    assert_eq!(row.sha256, expected64);
    measurements.push(row);
    cache.pause(false);
    let sequential: Vec<u64> = (0..8192).map(|i| i as u64 * BLOCK_BYTES as u64).collect();
    let baseline = measure(
        "HDD direct sequential 512 MiB",
        1,
        &sequential,
        BLOCK_BYTES,
        |offset, buffer| {
            direct.seek(SeekFrom::Start(offset))?;
            direct.read_exact(buffer)
        },
    )?;
    cache.clear();
    let sequential_cache = measure(
        "Cache sequential bypass 512 MiB",
        1,
        &sequential,
        BLOCK_BYTES,
        |offset, buffer| {
            assert_eq!(cache.read_at(offset, buffer)?, buffer.len());
            Ok(())
        },
    )?;
    assert_eq!(baseline.sha256, sequential_cache.sha256);
    measurements.extend([baseline, sequential_cache]);
    assert!(cache.statistics().sequential_bypasses > 0);
    // Functional checks use only this disposable source and disposable cache.
    cache.clear();
    let mut out = vec![0u8; BLOCK_BYTES];
    cache.read_at(0, &mut out)?;
    let mut changed = original.clone();
    changed[..4096].fill(0xa5);
    cache.write_source(0, &changed)?;
    cache.read_at(0, &mut out)?;
    assert_eq!(out, changed);
    cache.write_source(0, &original)?;
    cache.read_at(0, &mut out)?;
    assert_eq!(out, original);
    cache.clear();
    cache.read_at(0, &mut out)?;
    let mut corrupt = OpenOptions::new().write(true).open(&backing)?;
    corrupt.write_all(&[0xff; 4096])?;
    corrupt.sync_all()?;
    drop(corrupt);
    let before = cache.statistics().checksum_failures;
    cache.read_at(0, &mut out)?;
    assert_eq!(out, original);
    assert_eq!(cache.statistics().checksum_failures, before + 1);
    cache.resize(BLOCK_BYTES as u64)?;
    let evictions = cache.statistics().evictions;
    cache.read_at(0, &mut out)?;
    cache.read_at((BLOCK_BYTES * 2) as u64, &mut out)?;
    assert!(cache.statistics().evictions > evictions);
    cache.resize(capacity)?;
    assert_eq!(cache.occupancy(), 0);
    cache.gaming_policy(true);
    for block in 0..40 {
        cache.read_at(block * BLOCK_BYTES as u64, &mut out)?;
    }
    cache.clear();
    let report = serde_json::json!({"scope":"Controlled disposable-file API cache benchmark. NOT transparent system acceleration or driver qualification.","ioMode":"FILE_FLAG_NO_BUFFERING on source and SSD cache; 4096-byte aligned buffers","source":source,"backing":backing,"fixtureBytes":fixture_bytes,"capacityBytes":capacity,"workingSetBytes":offsets.len()*BLOCK_BYTES,"queueDepth":1,"rounds":3,"measurements":measurements,"checks":{"byteIdenticalSha256":true,"warmReadsAvoidedSourceApi":true,"pauseResume":true,"writeInvalidation":true,"corruptionFallback":true,"eviction":true,"resizeClear":true,"sequentialBypass":true,"gamingAdmissionPolicy":true},"statistics":cache.statistics(),"limitations":["Only reads issued through this harness use FileCache; applications and games are not accelerated.","No Windows file-data cache is used in measured reads, but drive firmware/controller caches remain active.","External writes outside the cache API are not monitored; source is disposable and exclusively controlled.","Gaming policy test covers admission only, not real game loading or automatic game discovery.","Capacity is reserved, benchmark data does not fill the entire cache."]});
    fs::write(
        cache_dir.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Report saved: {}", cache_dir.join("results.json").display());
    Ok(())
}
