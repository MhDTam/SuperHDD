//! Windows APIs only. Volume identities and dependency queries are read-only.
//! Container creation uses a new GUID-named file, never a physical source disk.
use crate::journal::{FileIdentity, Journal, OwnerRecord, Phase};
use crate::{same_volume, ContainerBudget, Dependency, VolumeIdentity};
use std::{
    fs::{File, OpenOptions},
    mem::{offset_of, size_of},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::{FileSystem::*, Vhd::*},
    System::{Com::CoCreateGuid, IO::DeviceIoControl},
};
#[path = "raw_volume.rs"]
mod raw_volume;
pub use raw_volume::RawVolume;
pub fn validate_single_partition(identity: &VolumeIdentity) -> Result<()> {
    raw_volume::single_partition(identity)
}
#[path = "lab_windows.rs"]
mod lab_windows;
pub use lab_windows::VerifiedLab;
#[path = "protected.rs"]
pub mod protected;
type Result<T> = std::result::Result<T, String>;
fn error(operation: &str, code: u32) -> String {
    format!(
        "{operation}: {}",
        std::io::Error::from_raw_os_error(code as i32)
    )
}
fn wide(value: &str) -> Result<Vec<u16>> {
    if value.contains('\0') {
        return Err("Embedded NUL is not a Windows path.".into());
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}
fn volume_guid_shape(text: &str) -> bool {
    text.strip_prefix("\\\\?\\Volume{")
        .and_then(|s| s.strip_suffix("}\\"))
        .is_some_and(|s| {
            s.len() == 36
                && s.bytes().enumerate().all(|(i, b)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
        })
}
pub fn volume_guid(mount: &str) -> Result<String> {
    if volume_guid_shape(mount) {
        return Ok(mount.to_string());
    }
    let bytes = mount.as_bytes();
    if !(bytes.len() == 2 || (bytes.len() == 3 && bytes[2] == b'\\'))
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
    {
        return Err(
            "Use a drive root or canonical volume GUID, not an arbitrary device path.".into(),
        );
    }
    let root = wide(&format!("{}:\\", bytes[0] as char))?;
    let mut buffer = [0u16; 128];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(root.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32)
    } == 0
    {
        return Err(error("GetVolumeNameForVolumeMountPoint", unsafe {
            GetLastError()
        }));
    }
    String::from_utf16(
        &buffer[..buffer
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated volume GUID")?],
    )
    .map_err(|e| e.to_string())
}
pub fn volume_device(mount: &str) -> Result<String> {
    let guid = volume_guid(mount)?;
    let name = wide(
        guid.strip_prefix("\\\\?\\")
            .ok_or("Invalid volume GUID")?
            .trim_end_matches('\\'),
    )?;
    let mut buffer = [0u16; 1024];
    if unsafe { QueryDosDeviceW(name.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) } == 0 {
        return Err(error("Resolve volume kernel name", unsafe {
            GetLastError()
        }));
    }
    let result = String::from_utf16(
        &buffer[..buffer
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated kernel name")?],
    )
    .map_err(|e| e.to_string())?;
    let suffix = result
        .strip_prefix("\\Device\\HarddiskVolume")
        .ok_or("Unsupported kernel volume name")?;
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid kernel volume name.".into());
    }
    Ok(result)
}
pub fn volume_filesystem(mount: &str) -> Result<String> {
    let guid = wide(&volume_guid(mount)?)?;
    let mut buffer = [0u16; 32];
    if unsafe {
        GetVolumeInformationW(
            guid.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } == 0
    {
        return Err(error("Query volume filesystem", unsafe { GetLastError() }));
    }
    String::from_utf16(
        &buffer[..buffer
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated filesystem")?],
    )
    .map_err(|e| e.to_string())
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
            null_mut(),
        )
    } == 0
    {
        return Err(error("Storage identity query", unsafe { GetLastError() }));
    }
    if returned as usize > output.len() {
        return Err("Oversized storage identity response.".into());
    }
    Ok(returned as usize)
}
pub fn volume_identity(mount: &str) -> Result<VolumeIdentity> {
    let volume_guid = volume_guid(mount)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(volume_guid.trim_end_matches('\\'))
        .map_err(|e| e.to_string())?;
    let mut extents = [0u8; 32];
    let n = ioctl(&file, 0x00560000, &[], &mut extents)?;
    if n != 32 || u32::from_le_bytes(extents[..4].try_into().unwrap()) != 1 {
        return Err("Only one proven disk extent is supported.".into());
    }
    let number = u32::from_le_bytes(extents[8..12].try_into().unwrap());
    let start = i64::from_le_bytes(extents[16..24].try_into().unwrap());
    let length = i64::from_le_bytes(extents[24..32].try_into().unwrap());
    if start < 0 || length <= 0 || (start as u64).checked_add(length as u64).is_none() {
        return Err("Invalid or overflowing volume extent.".into());
    }
    let mut descriptor = [0u8; 4096];
    let mut query = [0u8; 12];
    let n = ioctl(&file, 0x002d1400, &query, &mut descriptor)?;
    let declared = u32::from_le_bytes(descriptor[4..8].try_into().unwrap()) as usize;
    if n < 36 || declared < 36 || declared > n || descriptor[10] > 1 {
        return Err("Incomplete storage descriptor.".into());
    }
    let serial_offset = u32::from_le_bytes(descriptor[24..28].try_into().unwrap()) as usize;
    let serial = if serial_offset == 0 {
        String::new()
    } else {
        if serial_offset < 36 || serial_offset >= declared {
            return Err("Invalid serial offset.".into());
        }
        let bytes = &descriptor[serial_offset..declared];
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or("Unterminated storage serial")?;
        std::str::from_utf8(&bytes[..end])
            .map_err(|e| e.to_string())?
            .trim()
            .to_string()
    };
    query[..4].copy_from_slice(&7u32.to_le_bytes());
    let mut seek = [0u8; 12];
    let incurs_seek_penalty = match ioctl(&file, 0x002d1400, &query, &mut seek) {
        Ok(n)
            if n >= 12
                && u32::from_le_bytes(seek[4..8].try_into().unwrap()) >= 12
                && seek[8] <= 1 =>
        {
            Some(seek[8] != 0)
        }
        _ => None,
    };
    Ok(VolumeIdentity {
        volume_guid,
        disk_number: number,
        extent_offset: start as u64,
        extent_length: length as u64,
        bus_type: u32::from_le_bytes(descriptor[28..32].try_into().unwrap()),
        serial,
        removable: descriptor[10] != 0,
        incurs_seek_penalty,
    })
}

fn bounded_string(buffer: &[u64], used: usize, pointer: *const u16) -> Result<String> {
    if pointer.is_null() {
        return Ok(String::new());
    }
    let base = buffer.as_ptr() as usize;
    let offset = (pointer as usize)
        .checked_sub(base)
        .ok_or("Dependency string before output buffer")?;
    if offset % 2 != 0 || offset >= used {
        return Err("Dependency string outside output buffer.".into());
    }
    let available = (used - offset) / 2;
    let text = unsafe { std::slice::from_raw_parts(pointer, available.min(32768)) };
    let end = text
        .iter()
        .position(|c| *c == 0)
        .ok_or("Unterminated dependency string")?;
    String::from_utf16(&text[..end]).map_err(|e| e.to_string())
}
pub fn disk_dependencies(physical_disk: &str) -> Result<Vec<Dependency>> {
    let suffix = physical_disk
        .strip_prefix("\\\\.\\PhysicalDrive")
        .ok_or("Use a physical-disk path returned by VirtDisk")?;
    if suffix.is_empty() || suffix.len() > 10 || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid virtual-disk device identity.".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(physical_disk)
        .map_err(|e| e.to_string())?;
    // Aligned storage, bounded native pointers and exact returned-array ranges.
    let mut buffer = vec![0u64; 32768];
    let info = buffer.as_mut_ptr().cast::<STORAGE_DEPENDENCY_INFO>();
    unsafe {
        (*info).Version = STORAGE_DEPENDENCY_INFO_VERSION_2;
    }
    let mut used = 0;
    let code = unsafe {
        GetStorageDependencyInformation(
            file.as_raw_handle(),
            GET_STORAGE_DEPENDENCY_FLAG_HOST_VOLUMES | GET_STORAGE_DEPENDENCY_FLAG_DISK_HANDLE,
            (buffer.len() * 8) as u32,
            info,
            &mut used,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(error("GetStorageDependencyInformation", code));
    }
    let used = used as usize;
    if used < offset_of!(STORAGE_DEPENDENCY_INFO, Anonymous)
        || used > buffer.len() * 8
        || unsafe { (*info).Version } != STORAGE_DEPENDENCY_INFO_VERSION_2
    {
        return Err("Invalid dependency output size or version.".into());
    }
    let count = unsafe { (*info).NumberEntries } as usize;
    let start = offset_of!(STORAGE_DEPENDENCY_INFO, Anonymous);
    let end = count
        .checked_mul(size_of::<STORAGE_DEPENDENCY_INFO_TYPE_2>())
        .and_then(|n| n.checked_add(start))
        .ok_or("Dependency size overflow")?;
    if count > 64 || end > used {
        return Err("Invalid dependency array bounds.".into());
    }
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let pointer = unsafe {
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(start + index * size_of::<STORAGE_DEPENDENCY_INFO_TYPE_2>())
        };
        let entry =
            unsafe { std::ptr::read_unaligned(pointer.cast::<STORAGE_DEPENDENCY_INFO_TYPE_2>()) };
        let vendor = entry.VirtualStorageType.VendorId;
        let microsoft = VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT;
        result.push(Dependency {
            flags: entry.DependencyTypeFlags as u32,
            provider_flags: entry.ProviderSpecificFlags,
            device_id: entry.VirtualStorageType.DeviceId,
            microsoft_provider: vendor.data1 == microsoft.data1
                && vendor.data2 == microsoft.data2
                && vendor.data3 == microsoft.data3
                && vendor.data4 == microsoft.data4,
            ancestor_level: entry.AncestorLevel,
            dependency_device: bounded_string(&buffer, used, entry.DependencyDeviceName)?,
            host_volume: bounded_string(&buffer, used, entry.HostVolumeName)?,
            dependent_volume: bounded_string(&buffer, used, entry.DependentVolumeName)?,
            relative_path: bounded_string(&buffer, used, entry.DependentVolumeRelativePath)?,
        });
    }
    Ok(result)
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn guid_bytes(id: windows_sys::core::GUID) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&id.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&id.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&id.data3.to_le_bytes());
    bytes[8..].copy_from_slice(&id.data4);
    bytes
}
fn file_identity(file: &File) -> Result<FileIdentity> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(error("Query file identity", unsafe { GetLastError() }));
    }
    if info.nNumberOfLinks != 1 {
        return Err("Owned files must have exactly one hard link.".into());
    }
    Ok(FileIdentity {
        volume_serial: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        created: (u64::from(info.ftCreationTime.dwHighDateTime) << 32)
            | u64::from(info.ftCreationTime.dwLowDateTime),
    })
}
/// Stable NTFS file identity for an already pinned, validated service payload.
pub fn stable_file_identity(file: &File) -> Result<FileIdentity> {
    file_identity(file)
}
fn ordinary_file(file: &File) -> Result<()> {
    if file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_attributes()
        & (FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_COMPRESSED
            | FILE_ATTRIBUTE_ENCRYPTED
            | FILE_ATTRIBUTE_SPARSE_FILE)
        != 0
    {
        return Err("Owned files must be ordinary, fully allocated files.".into());
    }
    file_identity(file)?;
    Ok(())
}
fn pin_directories(root: &Path) -> Result<Vec<File>> {
    let mut directories = Vec::new();
    let mut prefix = PathBuf::new();
    for component in root.components() {
        prefix.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&prefix)
            .map_err(|e| e.to_string())?;
        if file
            .metadata()
            .map_err(|e| e.to_string())?
            .file_attributes()
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_ENCRYPTED)
            != 0
        {
            return Err(
                "Container directory ancestors must not be reparse, compressed or EFS encrypted."
                    .into(),
            );
        }
        directories.push(file);
    }
    Ok(directories)
}
fn vhd_info(
    handle: HANDLE,
    version: GET_VIRTUAL_DISK_INFO_VERSION,
) -> Result<GET_VIRTUAL_DISK_INFO> {
    let mut info = GET_VIRTUAL_DISK_INFO {
        Version: version,
        ..Default::default()
    };
    let mut length = size_of::<GET_VIRTUAL_DISK_INFO>() as u32;
    let mut used = 0;
    let code = unsafe { GetVirtualDiskInformation(handle, &mut length, &mut info, &mut used) };
    if code != ERROR_SUCCESS {
        return Err(error("GetVirtualDiskInformation", code));
    }
    if used as usize > size_of::<GET_VIRTUAL_DISK_INFO>() || info.Version != version {
        return Err("Invalid VHDX information response.".into());
    }
    Ok(info)
}
fn verify_vhd(handle: HANDLE, record: &OwnerRecord) -> Result<()> {
    let identifier = unsafe {
        vhd_info(handle, GET_VIRTUAL_DISK_INFO_VIRTUAL_DISK_ID)?
            .Anonymous
            .VirtualDiskId
    };
    let size = unsafe { vhd_info(handle, GET_VIRTUAL_DISK_INFO_SIZE)?.Anonymous.Size };
    let subtype = unsafe {
        vhd_info(handle, GET_VIRTUAL_DISK_INFO_PROVIDER_SUBTYPE)?
            .Anonymous
            .ProviderSubtype
    };
    let storage = unsafe {
        vhd_info(handle, GET_VIRTUAL_DISK_INFO_VIRTUAL_STORAGE_TYPE)?
            .Anonymous
            .VirtualStorageType
    };
    if record.vhd_id.is_some_and(|id| guid_bytes(identifier) != id)
        || guid_bytes(identifier) == [0; 16]
        || size.VirtualSize != record.budget.virtual_disk_bytes
        || size.SectorSize != 4096
        || subtype != 2
        || storage.DeviceId != VIRTUAL_STORAGE_TYPE_DEVICE_VHDX
        || guid_bytes(storage.VendorId) != guid_bytes(VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT)
    {
        return Err(format!("VHDX identity/layout differs: id={:02x?}, expected={:02x?}, bytes={}, sector={}, subtype={}, device={}, providerMatches={}",
            guid_bytes(identifier),record.vhd_id,size.VirtualSize,size.SectorSize,subtype,storage.DeviceId,
            guid_bytes(storage.VendorId)==guid_bytes(VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT)));
    }
    Ok(())
}
pub struct OwnedVhd {
    handle: Option<Handle>,
    file_pin: File,
    ownership: Journal,
    path: PathBuf,
    attached: bool,
    _directories: Vec<File>,
    pub budget: ContainerBudget,
    pub file_bytes: u64,
    pub parent: VolumeIdentity,
    lab: Option<VerifiedLab>,
}
impl OwnedVhd {
    /// Creates a new standalone fully-allocated VHDX. Does not format, partition,
    /// mount an existing user disk or load our cache driver. Must run as admin.
    pub fn create(root: &Path, budget: ContainerBudget) -> Result<Self> {
        Self::create_with_policy(root, budget, None, false)
    }
    /// Isolated test policy requires a verified firmware/VM capability. It is
    /// recorded in the journal and cannot be reopened through the direct API.
    pub fn create_in_lab(root: &Path, budget: ContainerBudget, lab: VerifiedLab) -> Result<Self> {
        lab.validate_root(root)?;
        if budget.total_bytes > 512 * 1024 * 1024 {
            return Err("Isolated container tests are limited to 512 MiB.".into());
        }
        Self::create_with_policy(root, budget, Some(lab), false)
    }
    pub fn create_in_lab_service(
        root: &Path,
        budget: ContainerBudget,
        lab: VerifiedLab,
    ) -> Result<Self> {
        lab.validate_root(root)?;
        let _private = protected::ProtectedDir::open(root)?;
        Self::create_with_policy(root, budget, Some(lab), true)
    }
    pub fn create_service(root: &Path, budget: ContainerBudget) -> Result<Self> {
        let _private = protected::ProtectedDir::open(root)?;
        Self::create_with_policy(root, budget, None, true)
    }
    fn create_with_policy(
        root: &Path,
        budget: ContainerBudget,
        lab: Option<VerifiedLab>,
        persistent_lease: bool,
    ) -> Result<Self> {
        let budget = ContainerBudget::new(budget.total_bytes)?;
        if lab.is_some() && budget.total_bytes > 512 * 1024 * 1024 {
            return Err("Lab container budget exceeds 512 MiB.".into());
        }
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        let mut directories = Vec::new();
        let mut prefix = PathBuf::new();
        for component in root.components() {
            prefix.push(component.as_os_str());
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&prefix)
                .map_err(|e| e.to_string())?;
            let attributes = file
                .metadata()
                .map_err(|e| e.to_string())?
                .file_attributes();
            if attributes
                & (FILE_ATTRIBUTE_REPARSE_POINT
                    | FILE_ATTRIBUTE_COMPRESSED
                    | FILE_ATTRIBUTE_ENCRYPTED)
                != 0
            {
                return Err("Container directory ancestors must not be reparse, compressed or EFS encrypted.".into());
            }
            directories.push(file);
        }
        let root_wide = wide(root.to_str().ok_or("Invalid container root")?)?;
        let mut mount = [0u16; 512];
        if unsafe { GetVolumePathNameW(root_wide.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) }
            == 0
        {
            return Err(error("Resolve container volume", unsafe { GetLastError() }));
        }
        let mount = String::from_utf16(
            &mount[..mount
                .iter()
                .position(|c| *c == 0)
                .ok_or("Unterminated mount path")?],
        )
        .map_err(|e| e.to_string())?;
        let parent = volume_identity(if volume_guid_shape(&mount) {
            &mount
        } else {
            mount.strip_prefix("\\\\?\\").unwrap_or(&mount)
        })?;
        if let Some(lab) = &lab {
            lab.validate_parent(&parent)?;
        } else if parent.removable
            || parent.serial.is_empty()
            || parent.incurs_seek_penalty != Some(false)
            || !matches!(parent.bus_type, 3 | 11 | 17)
        {
            return Err("Container requires a positively identified direct SSD volume.".into());
        }
        let guid = wide(&parent.volume_guid)?;
        let mut fs = [0u16; 16];
        if unsafe {
            GetVolumeInformationW(
                guid.as_ptr(),
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        } == 0
        {
            return Err(error("Query container filesystem", unsafe {
                GetLastError()
            }));
        }
        if String::from_utf16(
            &fs[..fs
                .iter()
                .position(|c| *c == 0)
                .ok_or("Unterminated filesystem name")?],
        )
        .map_err(|e| e.to_string())?
            != "NTFS"
        {
            return Err("Container requires an NTFS SSD volume.".into());
        }
        let mut available = 0;
        let mut total = 0;
        let mut free = 0;
        if unsafe { GetDiskFreeSpaceExW(root_wide.as_ptr(), &mut available, &mut total, &mut free) }
            == 0
        {
            return Err(error("Query SSD free-space reserve", unsafe {
                GetLastError()
            }));
        }
        let minimum_reserve = if lab.is_some() {
            64 * 1024 * 1024
        } else {
            10 * 1024u64.pow(3)
        };
        if budget.total_bytes > available.saturating_sub((total / 10).max(minimum_reserve)) {
            return Err("Container would consume the SSD free-space reserve.".into());
        }
        if std::fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
        {
            return Err("Use a new empty owned container directory.".into());
        }
        let mut id = windows_sys::core::GUID::default();
        if unsafe { CoCreateGuid(&mut id) } < 0 {
            return Err("Cannot generate a new container identity.".into());
        }
        let name = format!(
            "cache-{:08x}{:04x}{:04x}{}.vhdx",
            id.data1,
            id.data2,
            id.data3,
            id.data4
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let path = root.join(name);
        let ownership = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ)
            .open(root.join("owner.json"))
            .map_err(|e| e.to_string())?;
        let ownership = Journal::create(
            ownership,
            OwnerRecord {
                schema: 2,
                file: path.file_name().unwrap().to_string_lossy().into_owned(),
                instance: guid_bytes(id),
                budget,
                parent: parent.clone(),
                identity: None,
                vhd_id: None,
                phase: Phase::Creating,
                lab: lab.as_ref().map(|lab| lab.identity().clone()),
                persistent_lease,
            },
        )?;
        let path_wide = wide(path.to_str().ok_or("Invalid container path")?)?;
        let mut parameters = CREATE_VIRTUAL_DISK_PARAMETERS {
            Version: CREATE_VIRTUAL_DISK_VERSION_2,
            ..Default::default()
        };
        parameters.Anonymous.Version2 = CREATE_VIRTUAL_DISK_PARAMETERS_0_1 {
            UniqueId: id,
            MaximumSize: budget.virtual_disk_bytes,
            BlockSizeInBytes: 2 * 1024 * 1024,
            SectorSizeInBytes: 4096,
            PhysicalSectorSizeInBytes: 4096,
            ..Default::default()
        };
        let storage = VIRTUAL_STORAGE_TYPE {
            DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
            VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
        };
        let mut raw = null_mut();
        let code = unsafe {
            CreateVirtualDisk(
                &storage,
                path_wide.as_ptr(),
                VIRTUAL_DISK_ACCESS_NONE,
                null_mut(),
                CREATE_VIRTUAL_DISK_FLAG_FULL_PHYSICAL_ALLOCATION,
                0,
                &parameters,
                null(),
                &mut raw,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(error("CreateVirtualDisk (new file only)", code));
        }
        let handle = Handle(raw);
        let pin = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if pin == INVALID_HANDLE_VALUE {
            return Err(error("Pin new VHDX identity", unsafe { GetLastError() }));
        }
        let file_pin = unsafe { File::from_raw_handle(pin) };
        let attributes = file_pin
            .metadata()
            .map_err(|e| e.to_string())?
            .file_attributes();
        if attributes
            & (FILE_ATTRIBUTE_SPARSE_FILE
                | FILE_ATTRIBUTE_REPARSE_POINT
                | FILE_ATTRIBUTE_COMPRESSED
                | FILE_ATTRIBUTE_ENCRYPTED)
            != 0
        {
            return Err("New VHDX must be an ordinary fully allocated NTFS file.".into());
        }
        let mut value = Self {
            handle: Some(handle),
            file_pin,
            ownership,
            path,
            attached: false,
            _directories: directories,
            budget,
            file_bytes: 0,
            parent,
            lab,
        };
        value.file_bytes = std::fs::metadata(&value.path)
            .map_err(|e| e.to_string())?
            .len();
        value.budget.validate_file_size(value.file_bytes)?;
        verify_vhd(value.handle.as_ref().unwrap().0, value.ownership.record())?;
        let stable_id = unsafe {
            vhd_info(
                value.handle.as_ref().unwrap().0,
                GET_VIRTUAL_DISK_INFO_VIRTUAL_DISK_ID,
            )?
            .Anonymous
            .VirtualDiskId
        };
        value
            .ownership
            .created(file_identity(&value.file_pin)?, guid_bytes(stable_id))?;
        Ok(value)
    }
    /// Reopens only a recorded file identity on the same proven SSD. A partial
    /// initialization is preserved for diagnosis; it is never retried as a format.
    pub fn reopen(root: &Path) -> Result<Self> {
        Self::reopen_with_policy(root, None, false)
    }
    pub fn reopen_in_lab(root: &Path, lab: VerifiedLab) -> Result<Self> {
        lab.validate_root(root)?;
        Self::reopen_with_policy(root, Some(lab), false)
    }
    pub fn reopen_in_lab_service(root: &Path, lab: VerifiedLab) -> Result<Self> {
        lab.validate_root(root)?;
        let _private = protected::ProtectedDir::open(root)?;
        Self::reopen_with_policy(root, Some(lab), true)
    }
    pub fn reopen_service(root: &Path) -> Result<Self> {
        let _private = protected::ProtectedDir::open(root)?;
        Self::reopen_with_policy(root, None, true)
    }
    fn reopen_with_policy(
        root: &Path,
        lab: Option<VerifiedLab>,
        persistent_lease: bool,
    ) -> Result<Self> {
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        let directories = pin_directories(&root)?;
        let ownership = OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root.join("owner.json"))
            .map_err(|e| e.to_string())?;
        ordinary_file(&ownership)?;
        let ownership = Journal::open(ownership)?;
        let record = ownership.record();
        if record.persistent_lease != persistent_lease {
            return Err(
                "SCM-owned attachments require their protected service policy; preserved.".into(),
            );
        }
        if record.lab.as_ref() != lab.as_ref().map(|lab| lab.identity()) {
            return Err("Lab containers require their exact verified VM/run context; direct reopening was refused.".into());
        }
        if !matches!(record.phase, Phase::Created | Phase::RawReady) {
            return Err("Container creation/initialization was interrupted; preserving it without formatting or deletion.".into());
        }
        let parent = volume_identity(&record.parent.volume_guid)?;
        if !same_volume(&parent.volume_guid, &record.parent.volume_guid)
            || !parent.serial.eq_ignore_ascii_case(&record.parent.serial)
            || parent.extent_offset != record.parent.extent_offset
            || parent.extent_length != record.parent.extent_length
            || parent.bus_type != record.parent.bus_type
            || parent.removable
            || (lab.is_none() && parent.incurs_seek_penalty != Some(false))
        {
            return Err(
                "The container SSD identity or topology changed; preserving its files.".into(),
            );
        }
        if let Some(lab) = &lab {
            lab.validate_parent(&parent)?;
        }
        // Resolve the actual directory volume as well as the recorded volume;
        // moving the pair onto another volume cannot reuse the recorded proof.
        let directory_volume = directories.last().ok_or("Missing directory pin")?;
        let mut actual = vec![0u16; 32768];
        let count = unsafe {
            GetFinalPathNameByHandleW(
                directory_volume.as_raw_handle(),
                actual.as_mut_ptr(),
                actual.len() as u32,
                VOLUME_NAME_GUID,
            )
        };
        if count == 0 || count as usize >= actual.len() {
            return Err(error("Resolve recovered container root", unsafe {
                GetLastError()
            }));
        }
        let actual = String::from_utf16(&actual[..count as usize]).map_err(|e| e.to_string())?;
        if !actual
            .to_ascii_lowercase()
            .starts_with(&parent.volume_guid.to_ascii_lowercase())
        {
            return Err("Container directory is on a different volume.".into());
        }
        let path = root.join(&record.file);
        let file_pin = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .map_err(|e| e.to_string())?;
        ordinary_file(&file_pin)?;
        if Some(file_identity(&file_pin)?) != record.identity {
            return Err("The recorded VHDX file was replaced; preserving the replacement.".into());
        }
        let file_bytes = file_pin.metadata().map_err(|e| e.to_string())?.len();
        record.budget.validate_file_size(file_bytes)?;
        let budget = record.budget;
        let path_wide = wide(path.to_str().ok_or("Invalid recovered path")?)?;
        let storage = VIRTUAL_STORAGE_TYPE {
            DeviceId: VIRTUAL_STORAGE_TYPE_DEVICE_VHDX,
            VendorId: VIRTUAL_STORAGE_TYPE_VENDOR_MICROSOFT,
        };
        let parameters = OPEN_VIRTUAL_DISK_PARAMETERS {
            Version: OPEN_VIRTUAL_DISK_VERSION_2,
            ..Default::default()
        };
        let mut raw = null_mut();
        let code = unsafe {
            OpenVirtualDisk(
                &storage,
                path_wide.as_ptr(),
                VIRTUAL_DISK_ACCESS_NONE,
                OPEN_VIRTUAL_DISK_FLAG_NONE,
                &parameters,
                &mut raw,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(error("Open recorded VHDX", code));
        }
        let handle = Handle(raw);
        verify_vhd(handle.0, record)?;
        // Detect substitution between the file pin and the VirtDisk path open.
        let current = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
            .map_err(|e| e.to_string())?;
        if file_identity(&current)? != file_identity(&file_pin)? {
            return Err("Recovered VHDX path changed during open.".into());
        }
        Ok(Self {
            handle: Some(handle),
            file_pin,
            ownership,
            path,
            attached: false,
            _directories: directories,
            budget,
            file_bytes,
            parent,
            lab,
        })
    }
    pub(crate) fn validate_container_backing(&self, dependencies: &[Dependency]) -> Result<()> {
        match &self.lab {
            Some(lab) => lab.validate_backing(
                &self.parent,
                dependencies,
                self.ownership.record().persistent_lease,
            ),
            None => {
                if !self.ownership.record().persistent_lease {
                    return crate::validate_backing(&self.parent, dependencies);
                }
                let [dependency] = dependencies else {
                    return Err("Ambiguous service backing".into());
                };
                if dependency.flags & 1024 == 0 {
                    return Err("Service backing lost its persistent lifetime".into());
                }
                let mut ordinary = dependency.clone();
                ordinary.flags &= !1024;
                crate::validate_backing(&self.parent, &[ordinary])
            }
        }
    }
    pub fn phase(&self) -> Phase {
        self.ownership.record().phase
    }
    pub fn instance(&self) -> [u8; 16] {
        self.ownership.record().instance
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Query an existing attachment through this verified file handle. This
    /// acquires no lifetime lease and never detaches it in Drop.
    fn current_physical_path(&self) -> Result<Option<String>> {
        let mut path = [0u16; 256];
        let mut length = (path.len() * 2) as u32;
        let code = unsafe {
            GetVirtualDiskPhysicalPath(
                self.handle.as_ref().ok_or("Closed container")?.0,
                &mut length,
                path.as_mut_ptr(),
            )
        };
        if matches!(code, ERROR_DEV_NOT_EXIST | ERROR_NOT_FOUND) {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(error("Inspect recorded VHDX attachment", code));
        }
        let end = path
            .iter()
            .position(|v| *v == 0)
            .ok_or("Invalid attachment path")?;
        String::from_utf16(&path[..end])
            .map(Some)
            .map_err(|e| e.to_string())
    }
    /// Caller must first prove the kernel is detached. This only releases a
    /// previously observed attachment of this exact owned file.
    pub fn detach_observed_raw(&mut self, expected_disk: u32) -> Result<()> {
        let observed = self
            .inspect_attached_raw()?
            .ok_or("Recorded attachment disappeared")?;
        if observed.0 != expected_disk {
            return Err("Recorded attachment changed; preserved.".into());
        }
        self.attached = true;
        self.detach()
    }
    pub fn attach(&mut self) -> Result<String> {
        if self.attached {
            return Err("Container is already attached.".into());
        }
        let handle = self.handle.as_ref().ok_or("Closed container")?.0;
        let parameters = ATTACH_VIRTUAL_DISK_PARAMETERS {
            Version: ATTACH_VIRTUAL_DISK_VERSION_1,
            ..Default::default()
        };
        let code = unsafe {
            AttachVirtualDisk(
                handle,
                null_mut(),
                ATTACH_VIRTUAL_DISK_FLAG_NO_DRIVE_LETTER
                    | if self.ownership.record().persistent_lease {
                        ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME
                    } else {
                        0
                    },
                0,
                &parameters,
                null(),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(error("AttachVirtualDisk", code));
        }
        self.attached = true;
        let mut path = [0u16; 256];
        let mut bytes = (path.len() * 2) as u32;
        let code = unsafe { GetVirtualDiskPhysicalPath(handle, &mut bytes, path.as_mut_ptr()) };
        if code != ERROR_SUCCESS {
            return Err(error("GetVirtualDiskPhysicalPath", code));
        }
        let end = path
            .iter()
            .position(|c| *c == 0)
            .ok_or("Unterminated virtual-disk path")?;
        String::from_utf16(&path[..end]).map_err(|e| e.to_string())
    }
    pub fn detach(&mut self) -> Result<()> {
        if self.attached {
            let code = unsafe {
                DetachVirtualDisk(
                    self.handle.as_ref().ok_or("Closed container")?.0,
                    DETACH_VIRTUAL_DISK_FLAG_NONE,
                    0,
                )
            };
            if code != ERROR_SUCCESS {
                return Err(error("DetachVirtualDisk; preserving container", code));
            }
            self.attached = false;
        }
        Ok(())
    }
    /// Remove only this new object after a successful ordinary detach. Failure
    /// preserves the container; never force-detach or delete a mounted file.
    pub fn remove(mut self) -> Result<()> {
        self.detach()?;
        drop(self.handle.take());
        remove_created_file(&self.file_pin, &self.path)?;
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                self.ownership.file().as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(error("Remove owned container journal", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }
}
impl Drop for OwnedVhd {
    fn drop(&mut self) {
        let _ = self.detach();
    }
}

fn remove_created_file(pin: &File, path: &Path) -> Result<()> {
    let path = wide(path.to_str().ok_or("Invalid owned VHDX path")?)?;
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            DELETE | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(error("Open owned container for deletion", unsafe {
            GetLastError()
        }));
    }
    let target = Handle(raw);
    let mut original = BY_HANDLE_FILE_INFORMATION::default();
    let mut current = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(pin.as_raw_handle(), &mut original) } == 0
        || unsafe { GetFileInformationByHandle(target.0, &mut current) } == 0
    {
        return Err(error("Verify created container identity", unsafe {
            GetLastError()
        }));
    }
    if (
        original.dwVolumeSerialNumber,
        original.nFileIndexHigh,
        original.nFileIndexLow,
    ) != (
        current.dwVolumeSerialNumber,
        current.nFileIndexHigh,
        current.nFileIndexLow,
    ) || current.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err("Owned container path was replaced; preserving the replacement.".into());
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            target.0,
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(error("Remove verified owned container", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_api_refuses_lab_journal_before_opening_any_virtual_disk() {
        let lab = crate::lab::LabIdentity {
            vm_id: "0150b344-482c-4758-b3e4-9f4bbf2ec015".into(),
            run_id: "123456789abcdef0123456789abcdef0".into(),
            source_parent_volume: "\\\\?\\Volume{00000000-0000-0000-0000-000000000001}\\".into(),
            cache_parent_volume: "\\\\?\\Volume{00000000-0000-0000-0000-000000000002}\\".into(),
        };
        assert_direct_api_refuses_journal(Some(lab), false, "exact verified VM/run context");
    }
    #[test]
    fn direct_api_refuses_service_lease_before_opening_any_virtual_disk() {
        assert_direct_api_refuses_journal(None, true, "protected service policy");
    }
    fn assert_direct_api_refuses_journal(
        lab: Option<crate::lab::LabIdentity>,
        persistent_lease: bool,
        expected: &str,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("cache-123456789abcdef0123456789abcdef0.vhdx");
        std::fs::write(&path, b"must-not-be-opened-or-formatted").unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join("owner.json"))
            .unwrap();
        let mut journal = Journal::create(
            file,
            OwnerRecord {
                schema: 2,
                file: path.file_name().unwrap().to_str().unwrap().into(),
                instance: [1; 16],
                budget: ContainerBudget::new(256 * 1024 * 1024).unwrap(),
                parent: VolumeIdentity {
                    volume_guid: lab
                        .as_ref()
                        .map(|l| l.cache_parent_volume.clone())
                        .unwrap_or_else(|| "test-parent-volume".into()),
                    disk_number: 2,
                    extent_offset: 1048576,
                    extent_length: 1024 * 1024 * 1024,
                    bus_type: 1,
                    serial: "test-ssd".into(),
                    removable: false,
                    incurs_seek_penalty: None,
                },
                identity: None,
                vhd_id: None,
                phase: Phase::Creating,
                lab,
                persistent_lease,
            },
        )
        .unwrap();
        journal
            .created(
                FileIdentity {
                    volume_serial: 1,
                    index: 1,
                    created: 1,
                },
                [2; 16],
            )
            .unwrap();
        drop(journal);
        let error = OwnedVhd::reopen(directory.path()).err().unwrap();
        assert!(error.contains(expected), "{error}");
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"must-not-be-opened-or-formatted"
        );
    }
    #[test]
    fn replaced_container_path_is_preserved_by_file_identity_check() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("created.vhdx");
        let moved = directory.path().join("moved-created.vhdx");
        std::fs::write(&path, b"our-owned-fixture").unwrap();
        let pin = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        std::fs::rename(&path, &moved).unwrap();
        std::fs::write(&path, b"replacement-must-survive").unwrap();
        assert!(remove_created_file(&pin, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement-must-survive");
        assert_eq!(std::fs::read(&moved).unwrap(), b"our-owned-fixture");
    }
    #[test]
    fn original_created_file_is_removed_after_verified_handle_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("created.vhdx");
        std::fs::write(&path, b"our-owned-fixture").unwrap();
        let pin = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        remove_created_file(&pin, &path).unwrap();
        drop(pin);
        assert!(!path.exists());
    }
    #[test]
    fn dependency_strings_cannot_escape_returned_buffer() {
        let buffer = [0u64; 16];
        let base = buffer.as_ptr().cast::<u16>();
        assert_eq!(bounded_string(&buffer, 128, base).unwrap(), "");
        assert!(bounded_string(&buffer, 128, unsafe { base.add(64) }).is_err());
        assert!(bounded_string(&buffer, 128, (base as usize + 1) as *const u16).is_err());
        let no_terminator = [u64::MAX; 16];
        assert!(bounded_string(&no_terminator, 128, no_terminator.as_ptr().cast()).is_err());
    }
    #[test]
    fn arbitrary_device_paths_are_not_accepted_as_volume_choices() {
        assert!(volume_guid("\\\\.\\PhysicalDrive0").is_err());
        assert!(volume_guid("C:\\Windows").is_err());
        assert!(volume_guid("").is_err());
        assert!(volume_guid("\\\\?\\Volume{not-a-guid}\\").is_err());
    }
}
