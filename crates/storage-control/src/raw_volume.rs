//! Initializes only the blank virtual disk held by OwnedVhd. There is no entry
//! point accepting a caller-supplied physical disk number or device path.
use super::*;
use serde::Serialize;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Ioctl::*;
const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawVolume {
    pub volume_guid: String,
    pub device_path: String,
    pub disk_number: u32,
    pub capacity_bytes: u64,
}
fn signature(instance: [u8; 16]) -> u32 {
    u32::from_le_bytes(instance[..4].try_into().unwrap()) | 1
}
fn capacity(virtual_bytes: u64) -> Result<u64> {
    virtual_bytes
        .checked_sub(2 * MIB)
        .filter(|n| *n >= 2 * 65536 && n.is_multiple_of(65536))
        .ok_or_else(|| "Invalid owned RAW-volume size.".into())
}
fn blank_layout(style: u32, partitions: &[PARTITION_INFORMATION_EX], disk_signature: u32) -> bool {
    (style == PARTITION_STYLE_RAW as u32
        && partitions
            .iter()
            .all(|p| p.PartitionLength == 0 && p.StartingOffset == 0))
        || (style == PARTITION_STYLE_MBR as u32 && partitions.is_empty() && disk_signature == 0)
}
fn verify_blank_disk(file: &File, length: u64) -> Result<()> {
    #[repr(C, align(4096))]
    struct Sector([u8; 4096]);
    let mut sector = Sector([0; 4096]);
    let mut file = file.try_clone().map_err(|e| e.to_string())?;
    for offset in [
        0,
        length
            .checked_sub(4096)
            .ok_or("Invalid blank disk length")?,
    ] {
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        file.read_exact(&mut sector.0).map_err(|e| e.to_string())?;
        if sector.0.iter().any(|b| *b != 0) {
            return Err(
                "New owned virtual disk contains nonzero partition sectors; preserving it.".into(),
            );
        }
    }
    Ok(())
}
fn as_bytes<T>(value: &T) -> &[u8] {
    // All callers use fully zero-initialized Windows ABI structures. Padding is
    // initialized through the backing u64 allocation where a union is present.
    unsafe { std::slice::from_raw_parts((value as *const T).cast(), size_of::<T>()) }
}
fn layout(file: &File) -> Result<(u32, Vec<PARTITION_INFORMATION_EX>, u32)> {
    let mut memory = vec![0u64; 8192];
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(memory.as_mut_ptr().cast(), memory.len() * 8) };
    let n = ioctl(file, IOCTL_DISK_GET_DRIVE_LAYOUT_EX, &[], bytes)?;
    let head = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, PartitionEntry);
    if n < head {
        return Err("Truncated virtual-disk layout.".into());
    }
    let pointer = memory.as_ptr().cast::<DRIVE_LAYOUT_INFORMATION_EX>();
    let count = unsafe { (*pointer).PartitionCount };
    let style = unsafe { (*pointer).PartitionStyle };
    let required = head
        .checked_add(count as usize * size_of::<PARTITION_INFORMATION_EX>())
        .ok_or("Layout size overflow")?;
    if count > 128 || required > n {
        return Err("Oversized or truncated disk partition array.".into());
    }
    let partitions = unsafe {
        std::slice::from_raw_parts(
            bytes.as_ptr().add(head).cast::<PARTITION_INFORMATION_EX>(),
            count as usize,
        )
    }
    .to_vec();
    Ok((style, partitions, unsafe {
        (*pointer).Anonymous.Mbr.Signature
    }))
}
impl OwnedVhd {
    fn verified_disk(&self) -> Result<(File, u32)> {
        if !self.attached {
            return Err("Attach the owned container before querying its RAW volume.".into());
        }
        self.inspect_disk(false)
    }
    fn inspect_disk(&self, read_only: bool) -> Result<(File, u32)> {
        let handle = self.handle.as_ref().ok_or("Closed container")?.0;
        verify_vhd(handle, self.ownership.record())?;
        let mut wide_path = [0u16; 256];
        let mut length = (wide_path.len() * 2) as u32;
        let code =
            unsafe { GetVirtualDiskPhysicalPath(handle, &mut length, wide_path.as_mut_ptr()) };
        if code != ERROR_SUCCESS {
            return Err(error("Resolve owned virtual disk", code));
        }
        let path = String::from_utf16(
            &wide_path[..wide_path
                .iter()
                .position(|c| *c == 0)
                .ok_or("Invalid virtual path")?],
        )
        .map_err(|e| e.to_string())?;
        self.validate_container_backing(&disk_dependencies(&path)?)?;
        let file = OpenOptions::new()
            .read(true)
            .write(!read_only)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut descriptor = [0u8; 4096];
        let n = ioctl(
            &file,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &[0u8; 12],
            &mut descriptor,
        )?;
        if n < 36
            || u32::from_le_bytes(descriptor[28..32].try_into().unwrap())
                != BusTypeFileBackedVirtual as u32
        {
            return Err("Refusing initialization of a non-file-backed virtual device.".into());
        }
        let mut device = [0u8; 12];
        if ioctl(&file, IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], &mut device)? != 12 {
            return Err("Invalid virtual-device number response.".into());
        }
        let number = u32::from_le_bytes(device[4..8].try_into().unwrap());
        let mut bytes = [0u8; 8];
        if ioctl(&file, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut bytes)? != 8
            || u64::from_le_bytes(bytes) != self.budget.virtual_disk_bytes
            || path != format!("\\\\.\\PhysicalDrive{number}")
        {
            return Err("Owned virtual disk identity or length differs.".into());
        }
        Ok((file, number))
    }
    /// Read-only verification is allowed while a crashed service's RAW volume
    /// remains locked by the kernel. No volume/header open or detach occurs.
    pub fn inspect_attached_raw(&self) -> Result<Option<(u32, u64)>> {
        if self.current_physical_path()?.is_none() {
            return Ok(None);
        }
        let (file, number) = self.inspect_disk(true)?;
        self.check_layout(&file)?;
        Ok(Some((number, capacity(self.budget.virtual_disk_bytes)?)))
    }
    /// Creates one aligned MBR data partition inside this new, bounded VHDX.
    /// MBR is sufficient below the 25 GB budget and avoids GPT/MSR arrival races.
    /// The VirtDisk attachment suppresses drive letters for every volume.
    pub fn initialize_raw(&mut self) -> Result<RawVolume> {
        if self.phase() != Phase::Created {
            return Err(
                "Only a newly created, uninitialized owned container may be initialized.".into(),
            );
        }
        let (disk, number) = self.verified_disk()?;
        let (style, partitions, disk_signature) = layout(&disk)?;
        if !blank_layout(style, &partitions, disk_signature) {
            return Err(format!("Refusing to overwrite an existing virtual-disk partition layout (style {style}, entries {}).",partitions.len()));
        }
        verify_blank_disk(&disk, self.budget.virtual_disk_bytes)?;
        let bytes = capacity(self.budget.virtual_disk_bytes)?;
        self.ownership.advance(
            self.ownership.record().identity.clone(),
            Phase::Initializing,
        )?;
        let mut create_memory = [0u64; size_of::<CREATE_DISK>().div_ceil(8)];
        let create = unsafe { &mut *create_memory.as_mut_ptr().cast::<CREATE_DISK>() };
        create.PartitionStyle = PARTITION_STYLE_MBR;
        create.Anonymous.Mbr = CREATE_DISK_MBR {
            Signature: signature(self.instance()),
        };
        ioctl(&disk, IOCTL_DISK_CREATE_DISK, as_bytes(create), &mut [])?;
        const LAYOUT_BYTES: usize = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, PartitionEntry)
            + 4 * size_of::<PARTITION_INFORMATION_EX>();
        let mut memory = [0u64; LAYOUT_BYTES.div_ceil(8)];
        let proposed = unsafe { &mut *memory.as_mut_ptr().cast::<DRIVE_LAYOUT_INFORMATION_EX>() };
        proposed.PartitionStyle = PARTITION_STYLE_MBR as u32;
        proposed.PartitionCount = 4;
        proposed.Anonymous.Mbr = DRIVE_LAYOUT_INFORMATION_MBR {
            Signature: signature(self.instance()),
            CheckSum: 0,
        };
        let partition = &mut proposed.PartitionEntry[0];
        partition.PartitionStyle = PARTITION_STYLE_MBR;
        partition.StartingOffset = MIB as i64;
        partition.PartitionLength = bytes as i64;
        partition.PartitionNumber = 1;
        partition.RewritePartition = true;
        let mbr = unsafe { &mut partition.Anonymous.Mbr };
        mbr.PartitionType = 7;
        mbr.RecognizedPartition = true;
        mbr.HiddenSectors = (MIB / 4096) as u32;
        let layout_bytes =
            unsafe { std::slice::from_raw_parts(memory.as_ptr().cast(), LAYOUT_BYTES) };
        ioctl(&disk, IOCTL_DISK_SET_DRIVE_LAYOUT_EX, layout_bytes, &mut [])?;
        ioctl(&disk, IOCTL_DISK_UPDATE_PROPERTIES, &[], &mut [])?;
        disk.sync_all().map_err(|e| e.to_string())?;
        self.check_layout(&disk)?;
        drop(disk);
        let raw = find_volume(number, bytes)?;
        raw.header(self.instance(), true)?;
        self.ownership
            .advance(self.ownership.record().identity.clone(), Phase::RawReady)?;
        Ok(raw)
    }
    /// Verifies an existing layout and marker. Never initializes, clears or writes.
    pub fn raw_volume(&self) -> Result<RawVolume> {
        if self.phase() != Phase::RawReady {
            return Err("The owned container has no completed RAW initialization.".into());
        }
        let (disk, number) = self.verified_disk()?;
        self.check_layout(&disk)?;
        let raw = find_volume(number, capacity(self.budget.virtual_disk_bytes)?)?;
        raw.header(self.instance(), false)?;
        Ok(raw)
    }
    fn check_layout(&self, disk: &File) -> Result<()> {
        let (style, partitions, actual_signature) = layout(disk)?;
        if style != PARTITION_STYLE_MBR as u32 {
            return Err("Owned RAW partition style changed.".into());
        }
        let partition = *partitions.first().ok_or("Missing owned RAW partition")?;
        let mbr = unsafe { partition.Anonymous.Mbr };
        if style!=PARTITION_STYLE_MBR as u32 || partitions.len()!=4
            || actual_signature!=signature(self.instance())
            || partition.PartitionStyle!=PARTITION_STYLE_MBR || partition.PartitionNumber!=1
            || partition.StartingOffset!=MIB as i64 || partition.PartitionLength!=capacity(self.budget.virtual_disk_bytes)? as i64
            || mbr.PartitionType!=7 || mbr.BootIndicator
            || partitions[1..].iter().any(|p| unsafe{p.Anonymous.Mbr.PartitionType}!=0 || p.StartingOffset!=0 || p.PartitionLength!=0)
        { return Err("Owned RAW partition layout changed; refusing to rewrite it.".into()); }
        Ok(())
    }
}
pub(super) fn single_partition(identity: &VolumeIdentity) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(format!("\\\\.\\PhysicalDrive{}", identity.disk_number))
        .map_err(|e| e.to_string())?;
    let (_, parts, _) = layout(&file)?;
    let parts: Vec<_> = parts.iter().filter(|p| p.PartitionLength > 0).collect();
    if parts.len() != 1
        || parts[0].StartingOffset != identity.extent_offset as i64
        || parts[0].PartitionLength != identity.extent_length as i64
    {
        return Err(
            "Recovery requires one uniquely proven data partition on the source VHDX.".into(),
        );
    }
    Ok(())
}
struct VolumeSearch(HANDLE);
impl Drop for VolumeSearch {
    fn drop(&mut self) {
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}
fn find_volume(number: u32, bytes: u64) -> Result<RawVolume> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut buffer = [0u16; 128];
        let raw = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if raw == INVALID_HANDLE_VALUE {
            return Err(error("Find owned RAW volume", unsafe { GetLastError() }));
        }
        let search = VolumeSearch(raw);
        let mut found = None;
        for _ in 0..2048 {
            let guid = String::from_utf16(
                &buffer[..buffer
                    .iter()
                    .position(|c| *c == 0)
                    .ok_or("Unterminated volume name")?],
            )
            .map_err(|e| e.to_string())?;
            if let Ok(identity) = volume_identity(&guid) {
                if identity.disk_number == number
                    && identity.extent_offset == MIB
                    && identity.extent_length == bytes
                {
                    if found.is_some() {
                        return Err("Ambiguous owned RAW volume.".into());
                    }
                    let guid_wide = wide(&guid)?;
                    let mut mounts = [0u16; 1024];
                    let mut required = 0;
                    if unsafe {
                        GetVolumePathNamesForVolumeNameW(
                            guid_wide.as_ptr(),
                            mounts.as_mut_ptr(),
                            mounts.len() as u32,
                            &mut required,
                        )
                    } == 0
                    {
                        return Err(error("Verify RAW volume has no mount points", unsafe {
                            GetLastError()
                        }));
                    }
                    if mounts[0] != 0 {
                        return Err(
                            "Owned RAW volume unexpectedly has a drive letter or mount point."
                                .into(),
                        );
                    }
                    let dos = wide(
                        guid.strip_prefix("\\\\?\\")
                            .ok_or("Invalid GUID path")?
                            .trim_end_matches('\\'),
                    )?;
                    let mut device = [0u16; 1024];
                    if unsafe {
                        QueryDosDeviceW(dos.as_ptr(), device.as_mut_ptr(), device.len() as u32)
                    } == 0
                    {
                        return Err(error("Resolve RAW kernel device", unsafe {
                            GetLastError()
                        }));
                    }
                    let device_path = String::from_utf16(
                        &device[..device
                            .iter()
                            .position(|c| *c == 0)
                            .ok_or("Invalid kernel device")?],
                    )
                    .map_err(|e| e.to_string())?;
                    let suffix = device_path
                        .strip_prefix("\\Device\\HarddiskVolume")
                        .ok_or("Unexpected RAW volume kernel name")?;
                    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
                        return Err("Invalid RAW kernel name.".into());
                    }
                    found = Some(RawVolume {
                        volume_guid: guid,
                        device_path,
                        disk_number: number,
                        capacity_bytes: bytes,
                    });
                }
            }
            if unsafe { FindNextVolumeW(search.0, buffer.as_mut_ptr(), buffer.len() as u32) } == 0 {
                if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
                    return Err(error("Enumerate RAW volumes", unsafe { GetLastError() }));
                }
                break;
            }
        }
        if let Some(found) = found {
            return Ok(found);
        }
        if Instant::now() >= deadline {
            return Err("Owned RAW volume did not arrive within five seconds.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
impl RawVolume {
    fn header(&self, instance: [u8; 16], initialize: bool) -> Result<()> {
        // Sector-aligned stack buffer for volume I/O, including 4 KiB VHDX sectors.
        #[repr(C, align(4096))]
        struct Sector([u8; 4096]);
        let mut sector = Sector([0; 4096]);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH)
            .open(self.volume_guid.trim_end_matches('\\'))
            .map_err(|e| e.to_string())?;
        // A lock prevents a filesystem from claiming this volume during bootstrap.
        ioctl(&file, FSCTL_LOCK_VOLUME, &[], &mut [])?;
        let result = (|| {
            let mut extents = [0u8; 32];
            if ioctl(
                &file,
                IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
                &[],
                &mut extents,
            )? != 32
                || u32::from_le_bytes(extents[..4].try_into().unwrap()) != 1
                || u32::from_le_bytes(extents[8..12].try_into().unwrap()) != self.disk_number
                || u64::from_le_bytes(extents[16..24].try_into().unwrap()) != MIB
                || u64::from_le_bytes(extents[24..32].try_into().unwrap()) != self.capacity_bytes
            {
                return Err("RAW volume changed before its header check.".into());
            }
            file.read_exact(&mut sector.0).map_err(|e| e.to_string())?;
            if initialize {
                if sector.0.iter().any(|b| *b != 0) {
                    return Err("Refusing to overwrite a nonzero RAW volume header.".into());
                }
                sector.0 = crate::format::bootstrap(instance)?;
                file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                file.write_all(&sector.0).map_err(|e| e.to_string())?;
                file.sync_all().map_err(|e| e.to_string())?;
                sector.0.fill(0);
                file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                file.read_exact(&mut sector.0).map_err(|e| e.to_string())?;
            }
            crate::format::validate_header(&sector.0, instance, self.capacity_bytes)
        })();
        let unlock = ioctl(&file, FSCTL_UNLOCK_VOLUME, &[], &mut []);
        result?;
        unlock?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_blank_mbr_response_requires_no_partitions_or_signature() {
        assert!(blank_layout(PARTITION_STYLE_MBR as u32, &[], 0));
        assert!(!blank_layout(PARTITION_STYLE_MBR as u32, &[], 42));
        assert!(!blank_layout(
            PARTITION_STYLE_MBR as u32,
            &[Default::default()],
            0
        ));
        let mut partition = PARTITION_INFORMATION_EX {
            PartitionLength: 65536,
            ..Default::default()
        };
        assert!(!blank_layout(PARTITION_STYLE_RAW as u32, &[partition], 0));
        partition.PartitionLength = 0;
        assert!(blank_layout(PARTITION_STYLE_RAW as u32, &[partition], 0));
    }
    #[test]
    fn raw_partition_leaves_both_alignment_margins_inside_budget() {
        for total in [128 * MIB, 256 * MIB, crate::MAX_BUDGET_BYTES] {
            let budget = ContainerBudget::new(total).unwrap();
            let bytes = capacity(budget.virtual_disk_bytes).unwrap();
            assert_eq!(bytes % 65536, 0);
            assert_eq!(MIB + bytes + MIB, budget.virtual_disk_bytes);
        }
        assert!(capacity(MIB).is_err());
        assert!(capacity(3 * MIB + 1).is_err());
    }
}
