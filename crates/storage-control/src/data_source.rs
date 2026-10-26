//! Existing GPT data-volume handoff primitives. Callers must durably record
//! ownership and exclude encryption before handoff. This module never formats,
//! changes partition tables, forces an open file closed, or persists offline state.
use crate::{windows, VolumeIdentity};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    mem::{offset_of, size_of},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Storage::FileSystem::*,
        System::{Ioctl::*, IO::DeviceIoControl},
    },
};
type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskIdentity {
    pub number: u32,
    pub bytes: u64,
    pub logical_sector_bytes: u32,
    pub bus_type: u32,
    pub serial: String,
    pub gpt_id: [u8; 16],
    pub partition_id: [u8; 16],
    pub partition_offset: u64,
    pub partition_bytes: u64,
}
fn ioctl(file: &File, code: u32, input: &[u8], output: &mut [u8]) -> Result<usize> {
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            input.as_ptr().cast(),
            input.len() as u32,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "Storage control {code:#x}: {}",
            std::io::Error::last_os_error()
        ));
    }
    if returned as usize > output.len() {
        return Err("Oversized storage control response".into());
    }
    Ok(returned as usize)
}
fn open_disk(number: u32, write: bool) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(write)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(format!("\\\\.\\PhysicalDrive{number}"))
        .map_err(|e| e.to_string())
}
fn guid_bytes(guid: GUID) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..].copy_from_slice(&guid.data4);
    bytes
}
const BASIC: [u8; 16] = [
    0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7,
];
const MSR: [u8; 16] = [
    0x16, 0xe3, 0xc9, 0xe3, 0x5c, 0x0b, 0xb8, 0x4d, 0x81, 0x7d, 0xf9, 0x2d, 0xf0, 0x02, 0x15, 0xae,
];
fn check_partitions(
    parts: &[PARTITION_INFORMATION_EX],
    bytes: u64,
    offset: u64,
    length: u64,
) -> Result<[u8; 16]> {
    let mut selected = None;
    let mut msr = None;
    for part in parts {
        if part.PartitionLength == 0 {
            continue;
        }
        if part.PartitionStyle != PARTITION_STYLE_GPT
            || part.StartingOffset < 0
            || part.PartitionLength < 0
        {
            return Err("Require ordinary GPT data partitions only".into());
        }
        let start = part.StartingOffset as u64;
        let size = part.PartitionLength as u64;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= bytes)
            .ok_or("Partition outside disk")?;
        let gpt = unsafe { part.Anonymous.Gpt };
        let kind = guid_bytes(gpt.PartitionType);
        if kind == BASIC
            && !part.IsServicePartition
            && start == offset
            && size == length
            && gpt.Attributes == 0
            && selected.is_none()
        {
            let id = guid_bytes(gpt.PartitionId);
            if id == [0; 16] {
                return Err("Missing data partition identity".into());
            }
            selected = Some(id);
        } else if kind == MSR && gpt.Attributes == 0 && msr.is_none() {
            msr = Some((start, end));
        } else {
            return Err("Additional data, system, recovery or special partitions prevent whole-disk handoff".into());
        }
    }
    let id = selected.ok_or("Selected volume is absent from disk layout")?;
    let end = offset.checked_add(length).ok_or("Volume extent overflow")?;
    if msr.is_some_and(|(start, finish)| start < end && offset < finish) {
        return Err("Overlapping source partitions".into());
    }
    Ok(id)
}
fn inspect(file: &File, number: u32, offset: u64, length: u64) -> Result<DiskIdentity> {
    let mut device = [0; 12];
    let mut size = [0; 8];
    let mut geometry = [0; 256];
    let mut descriptor = [0; 4096];
    if ioctl(file, IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], &mut device)? != 12
        || u32::from_le_bytes(device[4..8].try_into().unwrap()) != number
        || u32::from_le_bytes(device[..4].try_into().unwrap()) != 7
    {
        return Err("Disk number changed".into());
    }
    if ioctl(file, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut size)? != 8
        || ioctl(file, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geometry)? < 32
    {
        return Err("Incomplete disk geometry".into());
    }
    let bytes = u64::from_le_bytes(size);
    let logical_sector_bytes = u32::from_le_bytes(geometry[20..24].try_into().unwrap());
    if bytes == 0
        || bytes != u64::from_le_bytes(geometry[24..32].try_into().unwrap())
        || !matches!(logical_sector_bytes, 512 | 4096)
        || !bytes.is_multiple_of(logical_sector_bytes as u64)
    {
        return Err("Unsupported or inconsistent geometry".into());
    }
    let n = ioctl(
        file,
        IOCTL_STORAGE_QUERY_PROPERTY,
        &[0; 12],
        &mut descriptor,
    )?;
    let declared = u32::from_le_bytes(descriptor[4..8].try_into().unwrap()) as usize;
    if n < 36 || declared < 36 || declared > n || descriptor[10] != 0 {
        return Err("Removable or malformed source device".into());
    }
    let bus_type = u32::from_le_bytes(descriptor[28..32].try_into().unwrap());
    let serial_offset = u32::from_le_bytes(descriptor[24..28].try_into().unwrap()) as usize;
    let serial = if serial_offset == 0 {
        String::new()
    } else {
        if serial_offset < 36 || serial_offset >= declared {
            return Err("Invalid disk serial pointer".into());
        }
        let data = &descriptor[serial_offset..declared];
        let end = data
            .iter()
            .position(|b| *b == 0)
            .ok_or("Unterminated disk serial")?;
        std::str::from_utf8(&data[..end])
            .map_err(|e| e.to_string())?
            .trim()
            .to_string()
    };
    let mut memory = vec![0u64; 8192];
    let output = unsafe {
        std::slice::from_raw_parts_mut(memory.as_mut_ptr().cast::<u8>(), memory.len() * 8)
    };
    let n = ioctl(file, IOCTL_DISK_GET_DRIVE_LAYOUT_EX, &[], output)?;
    let head = offset_of!(DRIVE_LAYOUT_INFORMATION_EX, PartitionEntry);
    if n < head {
        return Err("Truncated GPT layout".into());
    }
    let layout = unsafe { &*memory.as_ptr().cast::<DRIVE_LAYOUT_INFORMATION_EX>() };
    let count = layout.PartitionCount as usize;
    if layout.PartitionStyle != PARTITION_STYLE_GPT as u32
        || count > 128
        || head + count * size_of::<PARTITION_INFORMATION_EX>() > n
    {
        return Err("Unsupported GPT partition array".into());
    }
    let parts = unsafe {
        std::slice::from_raw_parts(
            memory
                .as_ptr()
                .cast::<u8>()
                .add(head)
                .cast::<PARTITION_INFORMATION_EX>(),
            count,
        )
    };
    let partition_id = check_partitions(parts, bytes, offset, length)?;
    let gpt_id = guid_bytes(unsafe { layout.Anonymous.Gpt.DiskId });
    if gpt_id == [0; 16] {
        return Err("Missing GPT disk identity".into());
    }
    Ok(DiskIdentity {
        number,
        bytes,
        logical_sector_bytes,
        bus_type,
        serial,
        gpt_id,
        partition_id,
        partition_offset: offset,
        partition_bytes: length,
    })
}
pub fn inspect_volume(volume: &VolumeIdentity) -> Result<DiskIdentity> {
    let disk = open_disk(volume.disk_number, false)?;
    let identity = inspect(
        &disk,
        volume.disk_number,
        volume.extent_offset,
        volume.extent_length,
    )?;
    if identity.bus_type != volume.bus_type || identity.serial != volume.serial || volume.removable
    {
        return Err("Source device identity changed".into());
    }
    Ok(identity)
}
/// Verify the one LUN belonging to an authenticated owned session before any
/// online/mount operation. Its GPT and extent must be the original bytes.
pub fn inspect_projection(source: &DiskIdentity, number: u32, run: &str) -> Result<DiskIdentity> {
    crate::iscsi::managed_target_name(run)?;
    if number == source.number { return Err("Projection cannot be the authoritative source".into()); }
    let disk = open_disk(number, false)?;
    let identity = inspect(&disk, number, source.partition_offset, source.partition_bytes)?;
    if identity.bus_type != 9 || identity.serial.trim() != run
        || identity.bytes != source.bytes || identity.logical_sector_bytes != source.logical_sector_bytes
        || identity.gpt_id != source.gpt_id || identity.partition_id != source.partition_id {
        return Err("Projected disk does not match the owned source/session".into());
    }
    Ok(identity)
}
fn attributes(disk: &File) -> Result<u64> {
    let mut buffer = [0; 16];
    if ioctl(disk, IOCTL_DISK_GET_DISK_ATTRIBUTES, &[], &mut buffer)? != 16 {
        return Err("Incomplete disk attributes".into());
    }
    Ok(u64::from_le_bytes(buffer[8..16].try_into().unwrap()))
}
fn set_offline(disk: &File, offline: bool) -> Result<()> {
    // Fully initialized ABI bytes; Persist=false and mask changes OFFLINE only.
    let mut buffer = [0u8; 40];
    buffer[..4].copy_from_slice(&(size_of::<SET_DISK_ATTRIBUTES>() as u32).to_le_bytes());
    buffer[8..16].copy_from_slice(&u64::from(offline).to_le_bytes());
    buffer[16..24].copy_from_slice(&1u64.to_le_bytes());
    ioctl(disk, IOCTL_DISK_SET_DISK_ATTRIBUTES, &buffer, &mut [])?;
    ioctl(disk, IOCTL_DISK_UPDATE_PROPERTIES, &[], &mut [])?;
    if (attributes(disk)? & 1 != 0) != offline {
        return Err("Disk state change was not confirmed".into());
    }
    Ok(())
}
/// The volume lock remains alive until the disk is confirmed offline. A lock
/// failure (including open files/pagefile/system volume) causes no dismount.
/// Caller must checkpoint recovery ownership before invoking this operation.
pub fn lock_and_offline(volume: &VolumeIdentity, expected: &DiskIdentity) -> Result<()> {
    if &inspect_volume(volume)? != expected {
        return Err("Source identity differs from recovery record".into());
    }
    if !windows::volume_filesystem(&volume.volume_guid)?.eq_ignore_ascii_case("NTFS") {
        return Err("Require NTFS source".into());
    }
    let disk = open_disk(expected.number, true)?;
    if attributes(&disk)? & 3 != 0 {
        return Err("Source must be online and writable before locking".into());
    }
    let guid = windows::volume_guid(&volume.volume_guid)?;
    let locked = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(guid.trim_end_matches('\\'))
        .map_err(|e| e.to_string())?;
    let mut extents = [0; 32];
    if ioctl(&locked, 0x00560000, &[], &mut extents)? != 32
        || u32::from_le_bytes(extents[..4].try_into().unwrap()) != 1
        || u32::from_le_bytes(extents[8..12].try_into().unwrap()) != expected.number
        || u64::from_le_bytes(extents[16..24].try_into().unwrap()) != expected.partition_offset
        || u64::from_le_bytes(extents[24..32].try_into().unwrap()) != expected.partition_bytes
    {
        return Err("Volume extent changed before locking".into());
    }
    ioctl(&locked, FSCTL_LOCK_VOLUME, &[], &mut [])
        .map_err(|error| format!("Source busy; close its open files before handoff: {error}"))?;
    locked.sync_all().map_err(|e| e.to_string())?;
    if inspect(
        &disk,
        expected.number,
        expected.partition_offset,
        expected.partition_bytes,
    )? != *expected
    {
        return Err("Disk changed while locking".into());
    }
    ioctl(&locked, FSCTL_DISMOUNT_VOLUME, &[], &mut [])?;
    set_offline(&disk, true)?;
    drop(locked);
    Ok(())
}
/// Explicit recovery only: caller must first prove no exported target can still
/// access this source. Drop deliberately never onlines a disk automatically.
pub fn restore_online(expected: &DiskIdentity) -> Result<()> {
    let disk = open_disk(expected.number, true)?;
    if inspect(
        &disk,
        expected.number,
        expected.partition_offset,
        expected.partition_bytes,
    )? != *expected
    {
        return Err("Recovery source identity changed".into());
    }
    set_offline(&disk, false)
}
/// Opens the authoritative disk only after a durable owner has locked and
/// offlined its filesystem. The caller keeps that offline state until this file
/// is closed and every projected session has been drained and disconnected.
pub fn open_offline_device(expected: &DiskIdentity) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH)
        .open(format!("\\\\.\\PhysicalDrive{}", expected.number))
        .map_err(|e| e.to_string())?;
    if inspect(
        &file,
        expected.number,
        expected.partition_offset,
        expected.partition_bytes,
    )? != *expected
        || attributes(&file)? & 3 != 1
    {
        return Err(
            "Authoritative source must retain its exact identity and offline/writable state".into(),
        );
    }
    Ok(file)
}
pub fn is_offline(expected: &DiskIdentity) -> Result<bool> {
    let disk = open_disk(expected.number, false)?;
    if inspect(
        &disk,
        expected.number,
        expected.partition_offset,
        expected.partition_bytes,
    )? != *expected
    {
        return Err("Disk identity changed before state query".into());
    }
    Ok(attributes(&disk)? & 1 != 0)
}

fn matches_volume(volume: &VolumeIdentity, expected: &DiskIdentity) -> bool {
    volume.disk_number == expected.number
        && volume.extent_offset == expected.partition_offset
        && volume.extent_length == expected.partition_bytes
}
/// Resolve an existing partition by device/extent/GPT identity. No formatting.
pub fn existing_volume(expected: &DiskIdentity) -> Result<VolumeIdentity> {
    let mut buffer = [0u16; 1024];
    let handle = unsafe { FindFirstVolumeW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error().to_string());
    }
    struct Enumeration(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Enumeration {
        fn drop(&mut self) {
            unsafe {
                FindVolumeClose(self.0);
            }
        }
    }
    let _enumeration = Enumeration(handle);
    let mut found = None;
    for _ in 0..256 {
        let end = buffer
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated volume enumeration")?;
        let name = String::from_utf16(&buffer[..end]).map_err(|e| e.to_string())?;
        if let Ok(volume) = windows::volume_identity(&name) {
            if matches_volume(&volume, expected) && inspect_volume(&volume)? == *expected {
                if found.is_some() {
                    return Err("Ambiguous projected filesystem identity".into());
                }
                found = Some(volume);
            }
        }
        buffer.fill(0);
        if unsafe { FindNextVolumeW(handle, buffer.as_mut_ptr(), buffer.len() as u32) } == 0 {
            let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if code != windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES {
                return Err(std::io::Error::from_raw_os_error(code as i32).to_string());
            }
            return found.ok_or_else(|| "Owned NTFS volume has not arrived".into());
        }
    }
    Err("Volume enumeration exceeded its bound".into())
}
/// Bind only an unused drive root. An existing mapping must already identify
/// this exact device/extent/GPT; foreign mappings are never removed or replaced.
pub fn ensure_letter(expected: &DiskIdentity, letter: char) -> Result<VolumeIdentity> {
    if !letter.is_ascii_uppercase() {
        return Err("One canonical upper-case drive letter required".into());
    }
    let root = format!("{letter}:\\");
    let wide_root: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
    let mut current = [0u16; 128];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(
            wide_root.as_ptr(),
            current.as_mut_ptr(),
            current.len() as u32,
        )
    } != 0
    {
        let volume = windows::volume_identity(&root)?;
        if matches_volume(&volume, expected) && inspect_volume(&volume)? == *expected {
            return Ok(volume);
        }
        return Err(
            "Requested drive letter already belongs to another device; mapping preserved".into(),
        );
    }
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_INVALID_DRIVE, ERROR_PATH_NOT_FOUND,
    };
    if !matches!(
        code,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_INVALID_DRIVE
    ) {
        return Err(std::io::Error::from_raw_os_error(code as i32).to_string());
    }
    let volume = existing_volume(expected)?;
    if windows::volume_filesystem(&volume.volume_guid)? != "NTFS" {
        return Err("Owned projected filesystem is not NTFS".into());
    }
    let wide_volume: Vec<u16> = volume.volume_guid.encode_utf16().chain(Some(0)).collect();
    if unsafe { SetVolumeMountPointW(wide_root.as_ptr(), wide_volume.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mounted = windows::volume_identity(&root)?;
    if !matches_volume(&mounted, expected) || inspect_volume(&mounted)? != *expected {
        return Err("Drive-root identity changed during binding".into());
    }
    Ok(mounted)
}
/// Remove only this recorded NTFS mount after the exact disk is offline, before
/// logout makes its device identity unavailable. No foreign mapping is changed.
pub fn release_letter(volume:&VolumeIdentity, expected:&DiskIdentity, letter:char)->Result<()> {
    if !letter.is_ascii_uppercase() || !matches_volume(volume,expected) || !is_offline(expected)? {
        return Err("An exact offline owned volume is required before releasing its letter".into());
    }
    let root:Vec<u16>=format!("{letter}:\\").encode_utf16().chain(Some(0)).collect();
    let mut current=[0u16;128];
    if unsafe{GetVolumeNameForVolumeMountPointW(root.as_ptr(),current.as_mut_ptr(),current.len() as u32)}==0 {
        let code=unsafe{windows_sys::Win32::Foundation::GetLastError()};
        if matches!(code,2|3|15){return Ok(());}
        return Err(std::io::Error::from_raw_os_error(code as i32).to_string());
    }
    let end=current.iter().position(|v|*v==0).ok_or("Unterminated mount identity")?;
    let name=String::from_utf16(&current[..end]).map_err(|e|e.to_string())?;
    if !name.eq_ignore_ascii_case(&volume.volume_guid){return Err("Drive root now belongs to another volume; mapping preserved".into());}
    if unsafe{DeleteVolumeMountPointW(root.as_ptr())}==0{return Err(std::io::Error::last_os_error().to_string());}
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn part(kind: [u8; 16], offset: i64, length: i64) -> PARTITION_INFORMATION_EX {
        let mut p = PARTITION_INFORMATION_EX {
            PartitionStyle: PARTITION_STYLE_GPT,
            StartingOffset: offset,
            PartitionLength: length,
            ..Default::default()
        };
        let gpt = PARTITION_INFORMATION_GPT {
            PartitionType: GUID {
                data1: u32::from_le_bytes(kind[..4].try_into().unwrap()),
                data2: u16::from_le_bytes(kind[4..6].try_into().unwrap()),
                data3: u16::from_le_bytes(kind[6..8].try_into().unwrap()),
                data4: kind[8..].try_into().unwrap(),
            },
            PartitionId: GUID {
                data1: 42,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            ..Default::default()
        };
        p.Anonymous.Gpt = gpt;
        p
    }
    #[test]
    fn whole_disk_rejects_other_data_system_special_and_overlapping_partitions() {
        let data = part(BASIC, 4096, 16384);
        let msr = part(MSR, 1024, 1024);
        assert!(check_partitions(&[msr, data], 32768, 4096, 16384).is_ok());
        let mut reserved_service = msr;
        reserved_service.IsServicePartition = true;
        assert!(check_partitions(&[reserved_service, data], 32768, 4096, 16384).is_ok());
        let mut system_data = data;
        system_data.IsServicePartition = true;
        assert!(check_partitions(&[system_data], 32768, 4096, 16384).is_err());
        for extra in [
            part(BASIC, 20480, 4096),
            part([1; 16], 20480, 4096),
            part(MSR, 4096, 1024),
        ] {
            assert!(check_partitions(&[data, extra], 32768, 4096, 16384).is_err());
        }
        let mut special = data;
        special.Anonymous.Gpt.Attributes = 1;
        assert!(check_partitions(&[special], 32768, 4096, 16384).is_err());
        assert!(check_partitions(&[data], 32768, 8192, 16384).is_err());
        assert!(check_partitions(&[data], 8192, 4096, 16384).is_err());
    }
}
