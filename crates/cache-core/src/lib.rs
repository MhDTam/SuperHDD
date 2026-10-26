//! SSD file-backed cache algorithm and disposable-file test harness.
//! This module does not intercept Windows I/O. The kernel filter owns that boundary.
//! Payloads live in the backing file; the only RAM payload is one transient I/O block.
use serde::{Deserialize, Serialize};
use std::{
    alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout},
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub const BLOCK_BYTES: usize = 64 * 1024;
const MAX_CAPACITY: u64 = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub hits: u64,
    pub misses: u64,
    pub source_read_bytes: u64,
    pub ssd_read_bytes: u64,
    pub fill_bytes: u64,
    pub evictions: u64,
    pub checksum_failures: u64,
    pub backing_errors: u64,
    pub invalidations: u64,
    pub sequential_bypasses: u64,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    source_block: u64,
    valid: usize,
    checksum: u64,
    referenced: bool,
}

/// Serialized by exclusive ownership; a service may wrap this in a Mutex.
/// Source changes MUST pass through write_source/invalidate. External source writes
/// are not observed in this user-mode harness, so it is not a transparent cache.
pub struct FileCache {
    source: File,
    backing: File,
    source_len: u64,
    capacity: u64,
    mapping: HashMap<u64, usize>,
    slots: Vec<Option<Entry>>,
    hand: usize,
    paused: bool,
    last_end: Option<u64>,
    sequential_bytes: u64,
    sequential_limit: u64,
    stats: Statistics,
    unbuffered: bool,
    read_only: bool,
    fixed_source_length: bool,
}

// Windows unbuffered I/O requires sector-aligned addresses. This also bounds
// transient payload memory to one 64 KiB block for both I/O modes.
struct BlockBuffer(std::ptr::NonNull<u8>);
impl BlockBuffer {
    fn new() -> Self {
        let layout = Layout::from_size_align(BLOCK_BYTES, 4096).unwrap();
        let pointer = unsafe { alloc_zeroed(layout) };
        Self(std::ptr::NonNull::new(pointer).unwrap_or_else(|| handle_alloc_error(layout)))
    }
}
impl std::ops::Deref for BlockBuffer {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.as_ptr(), BLOCK_BYTES) }
    }
}
impl std::ops::DerefMut for BlockBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0.as_ptr(), BLOCK_BYTES) }
    }
}
impl Drop for BlockBuffer {
    fn drop(&mut self) {
        unsafe {
            dealloc(
                self.0.as_ptr(),
                Layout::from_size_align(BLOCK_BYTES, 4096).unwrap(),
            );
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
fn slots_for(capacity: u64) -> io::Result<Vec<Option<Entry>>> {
    if capacity < BLOCK_BYTES as u64
        || capacity > MAX_CAPACITY
        || !capacity.is_multiple_of(BLOCK_BYTES as u64)
    {
        return Err(invalid(
            "Capacity must be a whole 64 KiB block, between 64 KiB and 1 TiB.",
        ));
    }
    let count = usize::try_from(capacity / BLOCK_BYTES as u64)
        .map_err(|_| invalid("Capacity too large."))?;
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(count)
        .map_err(|_| io::Error::other("Insufficient memory for bounded cache metadata."))?;
    slots.resize(count, None);
    Ok(slots)
}

impl FileCache {
    /// Takes ownership of an already validated, exclusively controlled device
    /// handle. The caller must keep its filesystems offline for this lifetime,
    /// validate device/backing identities, and open with unbuffered read/write I/O.
    /// This constructor neither selects a device nor changes its mount state.
    #[cfg(windows)]
    pub fn create_from_owned_device(source: File, source_len: u64, backing: &Path, capacity: u64) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        slots_for(capacity)?;
        if source_len==0 || !source_len.is_multiple_of(4096){return Err(invalid("Device length must be positive and 4096-byte aligned."));}
        let backing=OpenOptions::new().create_new(true).read(true).write(true)
            .custom_flags(0xA0000000).open(backing)?;
        backing.set_len(capacity)?;
        Self::from_owned_device_handles(source, source_len, backing, capacity)
    }
    /// Reuses an owned SSD allocation after restart, with no inherited mappings.
    /// The caller must validate and exclusively hold both identities, keep source
    /// filesystems offline, and supply unbuffered writable handles. Old payload
    /// bytes are never trusted, even if the previous process exited cleanly.
    #[cfg(windows)]
    pub fn from_owned_device_handles(source: File, source_len: u64, backing: File, capacity: u64) -> io::Result<Self> {
        let slots=slots_for(capacity)?;
        if source_len==0 || !source_len.is_multiple_of(4096){return Err(invalid("Device length must be positive and 4096-byte aligned."));}
        let metadata=backing.metadata()?;
        if !metadata.is_file() || metadata.len()!=capacity {return Err(invalid("Owned SSD file must retain its exact allocation length."));}
        Ok(Self{source,backing,source_len,capacity,mapping:HashMap::new(),slots,hand:0,paused:false,
            last_end:None,sequential_bytes:0,sequential_limit:8*1024*1024,stats:Statistics::default(),
            unbuffered:true,read_only:false,fixed_source_length:true})
    }
    /// Never overwrites an existing cache path. Metadata is intentionally discarded
    /// on restart until a driver-backed source generation can be validated.
    pub fn create(source: &Path, backing: &Path, capacity: u64) -> io::Result<Self> {
        Self::create_mode(source, backing, capacity, false, false, false)
    }

    /// Bypasses the Windows file-data cache for controlled disposable-file
    /// benchmarks. Does not intercept I/O from other applications.
    #[cfg(windows)]
    pub fn create_unbuffered(source: &Path, backing: &Path, capacity: u64) -> io::Result<Self> {
        Self::create_mode(source, backing, capacity, true, false, false)
    }

    /// A block provider must be the only reader/writer of its disposable source.
    /// Existing volumes cannot be enrolled through this file-only constructor.
    #[cfg(windows)]
    pub fn create_exclusive_unbuffered(
        source: &Path,
        backing: &Path,
        capacity: u64,
    ) -> io::Result<Self> {
        Self::create_mode(source, backing, capacity, true, false, true)
    }

    /// Read-only file-system providers must exclude external writers/deleters for
    /// the entire cache lifetime. Fail opening if a writer already holds the file.
    /// This is a per-file lease, not transparent caching of an existing volume.
    #[cfg(windows)]
    pub fn create_read_only_unbuffered(
        source: &Path,
        backing: &Path,
        capacity: u64,
    ) -> io::Result<Self> {
        Self::create_mode(source, backing, capacity, true, true, false)
    }

    fn create_mode(
        source: &Path,
        backing: &Path,
        capacity: u64,
        unbuffered: bool,
        read_only: bool,
        exclusive: bool,
    ) -> io::Result<Self> {
        let slots = slots_for(capacity)?;
        let mut source_options = OpenOptions::new();
        source_options.read(true).write(!read_only);
        let mut backing_options = OpenOptions::new();
        backing_options.create_new(true).read(true).write(true);
        #[cfg(windows)]
        if unbuffered {
            use std::os::windows::fs::OpenOptionsExt;
            source_options.custom_flags(0x20000000); // FILE_FLAG_NO_BUFFERING
            backing_options.custom_flags(0x20000000);
        }
        #[cfg(windows)]
        if read_only {
            use std::os::windows::fs::OpenOptionsExt;
            source_options.share_mode(1); // FILE_SHARE_READ only, excluding writers and replacement.
        }
        #[cfg(windows)]
        if exclusive {
            use std::os::windows::fs::OpenOptionsExt;
            source_options.share_mode(0);
        }
        #[cfg(not(windows))]
        let _ = exclusive;
        let source = source_options.open(source)?;
        let source_len = source.metadata()?.len();
        if unbuffered && !source_len.is_multiple_of(4096) {
            return Err(invalid(
                "Unbuffered source length must be aligned to 4096 bytes.",
            ));
        }
        let backing = backing_options.open(backing)?;
        backing.set_len(capacity)?;
        Ok(Self {
            source,
            backing,
            source_len,
            capacity,
            mapping: HashMap::new(),
            slots,
            hand: 0,
            paused: false,
            last_end: None,
            sequential_bytes: 0,
            sequential_limit: 8 * 1024 * 1024,
            stats: Statistics::default(),
            unbuffered,
            read_only,
            fixed_source_length: false,
        })
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    pub fn occupancy(&self) -> u64 {
        self.mapping.len() as u64 * BLOCK_BYTES as u64
    }
    pub fn statistics(&self) -> &Statistics {
        &self.stats
    }
    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
    }
    pub fn gaming_policy(&mut self, enabled: bool) {
        self.sequential_limit = if enabled { 2 } else { 8 } * 1024 * 1024;
    }

    fn source_block(&mut self, block: u64, buffer: &mut [u8]) -> io::Result<usize> {
        let offset = block
            .checked_mul(BLOCK_BYTES as u64)
            .ok_or_else(|| invalid("Offset overflow."))?;
        if offset >= self.source_len {
            return Ok(0);
        }
        let length = (self.source_len - offset).min(BLOCK_BYTES as u64) as usize;
        self.source.seek(SeekFrom::Start(offset))?;
        self.source.read_exact(&mut buffer[..length])?;
        self.stats.source_read_bytes += length as u64;
        Ok(length)
    }

    fn next_slot(&mut self) -> usize {
        loop {
            let index = self.hand;
            self.hand = (self.hand + 1) % self.slots.len();
            match self.slots[index].as_mut() {
                None => return index,
                Some(entry) if entry.referenced => entry.referenced = false,
                Some(entry) => {
                    self.mapping.remove(&entry.source_block);
                    self.slots[index] = None;
                    self.stats.evictions += 1;
                    return index;
                }
            }
        }
    }

    fn forget(&mut self, block: u64) {
        if let Some(slot) = self.mapping.remove(&block) {
            self.slots[slot] = None;
            self.stats.invalidations += 1;
        }
    }

    pub fn invalidate(&mut self, offset: u64, length: u64) -> io::Result<()> {
        if length == 0 {
            return Ok(());
        }
        let end = offset
            .checked_add(length - 1)
            .ok_or_else(|| invalid("Range overflow."))?;
        // Iterate the bounded index rather than every block in a potentially huge write.
        let first = offset / BLOCK_BYTES as u64;
        let last = end / BLOCK_BYTES as u64;
        let affected: Vec<_> = self
            .mapping
            .keys()
            .copied()
            .filter(|block| *block >= first && *block <= last)
            .collect();
        for block in affected {
            self.forget(block);
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.mapping.clear();
        self.slots.fill(None);
        self.hand = 0;
        self.last_end = None;
        self.sequential_bytes = 0;
    }

    pub fn resize(&mut self, capacity: u64) -> io::Result<()> {
        let slots = slots_for(capacity)?;
        self.clear();
        self.backing.set_len(capacity)?;
        self.backing.sync_all()?;
        self.slots = slots;
        self.capacity = capacity;
        Ok(())
    }

    /// Reads arbitrary ranges with bounded transient RAM usage, including partial
    /// blocks. A corrupt/unreadable SSD block falls back to the authoritative source.
    pub fn read_at(&mut self, offset: u64, output: &mut [u8]) -> io::Result<usize> {
        offset
            .checked_add(output.len() as u64)
            .ok_or_else(|| invalid("Read range overflow."))?;
        if offset >= self.source_len || output.is_empty() {
            return Ok(0);
        }
        let length = (self.source_len - offset).min(output.len() as u64) as usize;
        if self.last_end == Some(offset) {
            self.sequential_bytes = self.sequential_bytes.saturating_add(length as u64);
        } else {
            self.sequential_bytes = length as u64;
        }
        self.last_end = Some(offset + length as u64);
        let admit = !self.paused && self.sequential_bytes <= self.sequential_limit;
        let mut buffer = BlockBuffer::new();
        let mut completed = 0;
        while completed < length {
            let position = offset + completed as u64;
            let block = position / BLOCK_BYTES as u64;
            let within = (position % BLOCK_BYTES as u64) as usize;
            let mut valid = 0;
            let mut hit = false;
            if !self.paused {
                if let Some(slot) = self.mapping.get(&block).copied() {
                    if let Some(entry) = self.slots[slot] {
                        let result = self
                            .backing
                            .seek(SeekFrom::Start(slot as u64 * BLOCK_BYTES as u64))
                            .and_then(|_| self.backing.read_exact(&mut buffer[..entry.valid]));
                        match result {
                            Ok(()) if checksum(&buffer[..entry.valid]) == entry.checksum => {
                                valid = entry.valid;
                                hit = true;
                                self.stats.hits += 1;
                                self.stats.ssd_read_bytes += valid as u64;
                                self.slots[slot].as_mut().unwrap().referenced = true;
                            }
                            Ok(()) => {
                                self.stats.checksum_failures += 1;
                                self.forget(block);
                            }
                            Err(_) => {
                                self.stats.backing_errors += 1;
                                self.forget(block);
                            }
                        }
                    }
                }
            }
            if !hit {
                self.stats.misses += 1;
                valid = self.source_block(block, &mut buffer)?;
                if admit {
                    let slot = self.next_slot();
                    let write = self
                        .backing
                        .seek(SeekFrom::Start(slot as u64 * BLOCK_BYTES as u64))
                        .and_then(|_| self.backing.write_all(&buffer[..valid]));
                    if write.is_ok() {
                        self.slots[slot] = Some(Entry {
                            source_block: block,
                            valid,
                            checksum: checksum(&buffer[..valid]),
                            referenced: true,
                        });
                        self.mapping.insert(block, slot);
                        self.stats.fill_bytes += valid as u64;
                    } else {
                        self.stats.backing_errors += 1;
                    }
                } else if !self.paused {
                    self.stats.sequential_bypasses += 1;
                }
            }
            if valid <= within {
                break;
            }
            let count = (valid - within).min(length - completed);
            output[completed..completed + count].copy_from_slice(&buffer[within..within + count]);
            completed += count;
        }
        Ok(completed)
    }

    /// Invalidates before writing; no authoritative write is acknowledged before
    /// the source file has accepted it and sync_data has completed.
    pub fn write_source(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        if self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Read-only source lease",
            ));
        }
        if self.unbuffered && (!offset.is_multiple_of(4096) || !bytes.len().is_multiple_of(4096)) {
            return Err(invalid("Unbuffered writes must be aligned to 4096 bytes."));
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| invalid("Write range overflow."))?;
        if self.fixed_source_length && end>self.source_len {
            return Err(invalid("Write exceeds the fixed device boundary."));
        }
        self.invalidate(offset, bytes.len() as u64)?;
        self.source.seek(SeekFrom::Start(offset))?;
        if self.unbuffered {
            let mut buffer = BlockBuffer::new();
            for chunk in bytes.chunks(BLOCK_BYTES) {
                buffer[..chunk.len()].copy_from_slice(chunk);
                self.source.write_all(&buffer[..chunk.len()])?;
            }
        } else {
            self.source.write_all(bytes)?;
        }
        self.source.sync_data()?;
        self.source_len = self.source_len.max(end);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    #[cfg(windows)]
    #[test]
    fn owned_device_handle_preserves_bounds_and_durable_partial_updates() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir=TempDir::new().unwrap();let path=dir.path().join("device-fixture.bin");
        std::fs::write(&path,vec![37;BLOCK_BYTES]).unwrap();
        let source=OpenOptions::new().read(true).write(true).share_mode(0).custom_flags(0xA0000000).open(&path).unwrap();
        let mut cache=FileCache::create_from_owned_device(source,BLOCK_BYTES as u64,&dir.path().join("cache.bin"),BLOCK_BYTES as u64).unwrap();
        let mut read=vec![0;BLOCK_BYTES];cache.read_at(0,&mut read).unwrap();assert!(read.iter().all(|b|*b==37));
        assert!(cache.write_source(BLOCK_BYTES as u64,&[9;4096]).is_err());
        cache.write_source(4096,&[91;4096]).unwrap();drop(cache);
        let actual=std::fs::read(&path).unwrap();assert_eq!(actual.len(),BLOCK_BYTES);
        assert!(actual[..4096].iter().all(|b|*b==37));assert!(actual[4096..8192].iter().all(|b|*b==91));assert!(actual[8192..].iter().all(|b|*b==37));
    }
    #[cfg(windows)]
    #[test]
    fn reusing_owned_ssd_allocation_discards_old_payload_and_mappings() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir=TempDir::new().unwrap();let source_path=dir.path().join("device.bin");let backing_path=dir.path().join("ssd.bin");
        std::fs::write(&source_path,vec![37;BLOCK_BYTES]).unwrap();
        let open=|path:&Path|OpenOptions::new().read(true).write(true).share_mode(0).custom_flags(0xA0000000).open(path).unwrap();
        let mut cache=FileCache::create_from_owned_device(open(&source_path),BLOCK_BYTES as u64,&backing_path,BLOCK_BYTES as u64).unwrap();
        let mut data=vec![0;BLOCK_BYTES];cache.read_at(0,&mut data).unwrap();assert_eq!(cache.occupancy(),BLOCK_BYTES as u64);drop(cache);
        // Mimic a stale or corrupted cache and an independently durable source.
        std::fs::write(&backing_path,vec![99;BLOCK_BYTES]).unwrap();
        std::fs::write(&source_path,vec![51;BLOCK_BYTES]).unwrap();
        let mut cache=FileCache::from_owned_device_handles(open(&source_path),BLOCK_BYTES as u64,open(&backing_path),BLOCK_BYTES as u64).unwrap();
        assert_eq!(cache.occupancy(),0);cache.read_at(0,&mut data).unwrap();assert!(data.iter().all(|b|*b==51));
        assert_eq!(cache.statistics().source_read_bytes,BLOCK_BYTES as u64);assert_eq!(cache.statistics().ssd_read_bytes,0);
        cache.read_at(0,&mut data).unwrap();assert_eq!(cache.statistics().ssd_read_bytes,BLOCK_BYTES as u64);drop(cache);
        assert!(FileCache::from_owned_device_handles(open(&source_path),BLOCK_BYTES as u64,open(&backing_path),2*BLOCK_BYTES as u64).is_err());
        assert_eq!(std::fs::metadata(&backing_path).unwrap().len(),BLOCK_BYTES as u64);
    }
    #[cfg(windows)]
    #[test]
    fn block_provider_excludes_source_handles_and_persists_writes_before_release() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.img");
        let backing = directory.path().join("cache.bin");
        std::fs::write(&source, vec![37; BLOCK_BYTES]).unwrap();
        let mut cache = FileCache::create_exclusive_unbuffered(
            &source, &backing, BLOCK_BYTES as u64,
        ).unwrap();
        assert!(File::open(&source).is_err());
        assert!(OpenOptions::new().write(true).open(&source).is_err());
        let mut output = [0; 4096];
        cache.read_at(0, &mut output).unwrap();
        cache.write_source(0, &[91; 4096]).unwrap();
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(output, [91; 4096]);
        drop(cache);
        assert_eq!(&std::fs::read(&source).unwrap()[..4096], &[91; 4096]);
    }
    #[cfg(windows)]
    #[test]
    fn read_only_provider_excludes_external_writes_and_replacement_until_release() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.bin");
        let backing = directory.path().join("cache.bin");
        std::fs::write(&source, vec![73; BLOCK_BYTES]).unwrap();
        let mut cache =
            FileCache::create_read_only_unbuffered(&source, &backing, BLOCK_BYTES as u64).unwrap();
        let mut data = [0; 512];
        assert_eq!(cache.read_at(0, &mut data).unwrap(), data.len());
        assert!(data.iter().all(|byte| *byte == 73));
        assert_eq!(
            cache.write_source(0, &[99; 4096]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(OpenOptions::new().write(true).open(&source).is_err());
        assert!(std::fs::remove_file(&source).is_err());
        drop(cache);
        assert!(OpenOptions::new().write(true).open(&source).is_ok());
        assert!(std::fs::read(&source)
            .unwrap()
            .iter()
            .all(|byte| *byte == 73));
    }
    #[cfg(windows)]
    #[test]
    fn read_only_provider_refuses_source_with_an_existing_writer_before_creating_cache() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.bin");
        let backing = directory.path().join("cache.bin");
        std::fs::write(&source, vec![73; BLOCK_BYTES]).unwrap();
        let _writer = OpenOptions::new().write(true).open(&source).unwrap();
        assert!(
            FileCache::create_read_only_unbuffered(&source, &backing, BLOCK_BYTES as u64).is_err()
        );
        assert!(!backing.exists());
    }
    #[test]
    fn mixed_read_write_eviction_resize_and_pause_trace_matches_source_oracle() {
        let (_directory, mut cache, mut expected) = setup(64, 8);
        let mut random = 0x21b7a9d45678e913u64;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            random
        };
        for operation in 0..4096 {
            if operation % 257 == 0 {
                cache
                    .resize((BLOCK_BYTES * (4 + next() as usize % 12)) as u64)
                    .unwrap();
            }
            if operation % 61 == 0 {
                cache.clear();
            }
            if operation % 37 == 0 {
                cache.pause(next() % 2 == 0);
            }
            let offset = next() as usize % expected.len();
            if operation % 13 == 0 {
                let count = (1 + next() as usize % 1024).min(expected.len() - offset);
                let bytes = vec![(next() % 251) as u8; count];
                cache.write_source(offset as u64, &bytes).unwrap();
                expected[offset..offset + count].copy_from_slice(&bytes);
            } else {
                let count = 1 + next() as usize % (BLOCK_BYTES + 127);
                let mut bytes = vec![0; count];
                let completed = cache.read_at(offset as u64, &mut bytes).unwrap();
                let actual = (expected.len() - offset).min(count);
                assert_eq!(completed, actual);
                assert_eq!(
                    &bytes[..completed],
                    &expected[offset..offset + actual],
                    "operation {operation}"
                );
            }
            assert!(cache.occupancy() <= cache.capacity());
        }
    }
    #[cfg(windows)]
    #[test]
    fn unbuffered_partial_reads_and_aligned_write_invalidation_are_byte_correct() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.bin");
        let backing = directory.path().join("cache.bin");
        let mut expected = vec![0x39; BLOCK_BYTES * 2];
        std::fs::write(&source, &expected).unwrap();
        let mut cache =
            FileCache::create_unbuffered(&source, &backing, (BLOCK_BYTES * 2) as u64).unwrap();
        let mut out = vec![0; 4099];
        cache.read_at(13, &mut out).unwrap();
        assert_eq!(out, expected[13..13 + 4099]);
        let before = cache.statistics().source_read_bytes;
        cache.read_at(13, &mut out).unwrap();
        assert_eq!(cache.statistics().source_read_bytes, before);
        cache.write_source(4096, &[0xa5; 4096]).unwrap();
        expected[4096..8192].fill(0xa5);
        cache.read_at(4090, &mut out).unwrap();
        assert_eq!(out, expected[4090..4090 + 4099]);
        assert_eq!(
            cache.write_source(1, &[0; 4096]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            cache
                .read_at((expected.len() - 7) as u64, &mut out)
                .unwrap(),
            7
        );
    }
    #[cfg(windows)]
    #[test]
    fn unbuffered_rejects_unaligned_fixture_without_creating_backing() {
        let directory = TempDir::new().unwrap();
        let source = directory.path().join("source.bin");
        let backing = directory.path().join("cache.bin");
        std::fs::write(&source, [0; 17]).unwrap();
        assert_eq!(
            FileCache::create_unbuffered(&source, &backing, BLOCK_BYTES as u64)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!backing.exists());
    }
    fn setup(blocks: usize, capacity: usize) -> (TempDir, FileCache, Vec<u8>) {
        let directory = TempDir::new().unwrap();
        let data: Vec<u8> = (0..blocks * BLOCK_BYTES + 19)
            .map(|i| ((i * 17 + i / BLOCK_BYTES) % 251) as u8)
            .collect();
        let source = directory.path().join("source.bin");
        std::fs::write(&source, &data).unwrap();
        let cache = FileCache::create(
            &source,
            &directory.path().join("cache.bin"),
            (capacity * BLOCK_BYTES) as u64,
        )
        .unwrap();
        (directory, cache, data)
    }
    #[test]
    fn second_read_is_ssd_served_without_source_api_read() {
        let (_dir, mut cache, data) = setup(4, 4);
        let mut output = vec![0; BLOCK_BYTES];
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(output, data[..BLOCK_BYTES]);
        let source_bytes = cache.statistics().source_read_bytes;
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(cache.statistics().hits, 1);
        assert_eq!(cache.statistics().source_read_bytes, source_bytes);
        assert_eq!(cache.statistics().ssd_read_bytes, BLOCK_BYTES as u64);
    }
    #[test]
    fn partial_write_invalidates_overlapping_blocks() {
        let (_dir, mut cache, mut expected) = setup(4, 4);
        let mut output = vec![0; BLOCK_BYTES * 2];
        cache.read_at(0, &mut output).unwrap();
        cache
            .write_source(BLOCK_BYTES as u64 - 2, &[9, 8, 7, 6])
            .unwrap();
        expected[BLOCK_BYTES - 2..BLOCK_BYTES + 2].copy_from_slice(&[9, 8, 7, 6]);
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(output, expected[..BLOCK_BYTES * 2]);
        assert_eq!(cache.statistics().invalidations, 2);
    }
    #[test]
    fn arbitrary_offsets_partial_hits_and_eof() {
        let (_dir, mut cache, data) = setup(3, 4);
        let mut output = vec![0; BLOCK_BYTES * 2 + 7];
        let n = cache.read_at(13, &mut output).unwrap();
        assert_eq!(&output[..n], &data[13..13 + n]);
        let n = cache.read_at((data.len() - 9) as u64, &mut output).unwrap();
        assert_eq!(n, 9);
        assert_eq!(&output[..9], &data[data.len() - 9..]);
        assert_eq!(
            cache.read_at(u64::MAX, &mut output).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    #[test]
    fn clock_evicts_and_capacity_is_enforced() {
        let (_dir, mut cache, data) = setup(8, 2);
        let mut output = vec![0; BLOCK_BYTES];
        for block in 0..8 {
            cache
                .read_at((block * BLOCK_BYTES) as u64, &mut output)
                .unwrap();
            assert_eq!(output, data[block * BLOCK_BYTES..(block + 1) * BLOCK_BYTES]);
            assert!(cache.occupancy() <= cache.capacity());
        }
        assert!(cache.statistics().evictions >= 6);
    }
    #[test]
    fn corrupted_ssd_falls_back_to_source() {
        let (dir, mut cache, data) = setup(2, 2);
        let mut output = vec![0; 100];
        cache.read_at(0, &mut output).unwrap();
        let mut backing = OpenOptions::new()
            .write(true)
            .open(dir.path().join("cache.bin"))
            .unwrap();
        backing.write_all(&[255; 100]).unwrap();
        backing.sync_all().unwrap();
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(output, data[..100]);
        assert_eq!(cache.statistics().checksum_failures, 1);
    }
    #[test]
    fn pause_resume_resize_and_clear() {
        let (_dir, mut cache, _) = setup(5, 4);
        let mut output = vec![0; 100];
        cache.read_at(0, &mut output).unwrap();
        cache.pause(true);
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(cache.statistics().hits, 0);
        cache.pause(false);
        cache.read_at(0, &mut output).unwrap();
        assert_eq!(cache.statistics().hits, 1);
        cache.resize((2 * BLOCK_BYTES) as u64).unwrap();
        assert_eq!(cache.occupancy(), 0);
        assert_eq!(cache.capacity(), (2 * BLOCK_BYTES) as u64);
        cache.read_at(0, &mut output).unwrap();
        cache.clear();
        assert_eq!(cache.occupancy(), 0);
    }
    #[test]
    fn refuses_to_overwrite_existing_backing_file() {
        let (dir, _cache, _) = setup(2, 2);
        let path = dir.path().join("cache.bin");
        assert_eq!(
            FileCache::create(&dir.path().join("source.bin"), &path, BLOCK_BYTES as u64)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }
    #[test]
    fn rejects_invalid_capacities_and_overflow() {
        assert!(slots_for(0).is_err());
        assert!(slots_for(BLOCK_BYTES as u64 + 1).is_err());
        assert!(slots_for(MAX_CAPACITY + 1).is_err());
        let (_dir, mut cache, _) = setup(2, 2);
        assert!(cache.invalidate(u64::MAX, 2).is_err());
    }
    #[test]
    fn long_sequential_reads_do_not_evict_the_entire_hot_set() {
        let (_dir, mut cache, _) = setup(160, 4);
        cache.gaming_policy(true);
        let mut output = vec![0; BLOCK_BYTES];
        for block in 0..100 {
            cache
                .read_at((block * BLOCK_BYTES) as u64, &mut output)
                .unwrap();
        }
        assert!(cache.statistics().sequential_bypasses > 0);
    }
}
