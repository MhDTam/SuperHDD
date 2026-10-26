//! Only a new run-owned VHDX is accepted. Physical HDDs cannot pass this guard.
use super::*;
use std::{
    fs::File,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
};
use superhdd_storage_control::windows::{disk_dependencies, volume_guid};
use windows_sys::Win32::System::{Ioctl::*, IO::DeviceIoControl};
fn ioctl(file: &File, code: u32, output: &mut [u8]) -> io::Result<usize> {
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            std::ptr::null(),
            0,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(returned as usize)
}
pub fn open(
    run: &str,
    root: &std::path::Path,
    number: u32,
    recovered: bool,
) -> io::Result<(
    FileCache,
    Option<superhdd_storage_control::windows::protected::ProtectedDir>,
)> {
    let source = PathBuf::from(format!("D:\\SuperHDD-handoff-{run}.vhdx"));
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("handoff-source-owner.json"))?)?;
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || saved["runId"] != run
        || saved["created"] != metadata.creation_time()
        || saved["sourceBytes"] != SOURCE_BYTES
    {
        return Err(invalid());
    }
    let path = format!("\\\\.\\PhysicalDrive{number}");
    let dependencies = disk_dependencies(&path).map_err(io::Error::other)?;
    let [dependency] = dependencies.as_slice() else {
        return Err(invalid());
    };
    let parent = volume_guid("D:").map_err(io::Error::other)?;
    if !dependency.microsoft_provider
        || dependency.device_id != 3
        || dependency.ancestor_level != 0
        || dependency.provider_flags != 0
        || !dependency
            .host_volume
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(parent.trim_end_matches('\\'))
        || !dependency
            .relative_path
            .eq_ignore_ascii_case(&format!("\\SuperHDD-handoff-{run}.vhdx"))
    {
        return Err(io::Error::other(
            "Only the run-owned VHDX device is accepted",
        ));
    }
    let native_owner = root.join("native-source.json");
    if native_owner.exists() {
        use std::io::Read;
        use superhdd_storage_control::{
            data_source::{self, DiskIdentity},
            journal::FileIdentity,
            windows::{protected::ProtectedDir, stable_file_identity},
        };
        let identity: DiskIdentity = serde_json::from_slice(&fs::read(native_owner)?)?;
        if identity.number != number || identity.bytes != SOURCE_BYTES {
            return Err(invalid());
        }
        let file = data_source::open_offline_device(&identity).map_err(io::Error::other)?;
        let private_path = root.join("private-ssd");
        let pins = if recovered {
            ProtectedDir::open(&private_path)
        } else {
            ProtectedDir::create(&private_path)
        }
        .map_err(io::Error::other)?;
        let backing = pins
            .payload("payload.bin", !recovered)
            .map_err(io::Error::other)?;
        let file_id = stable_file_identity(&backing).map_err(io::Error::other)?;
        if recovered {
            let mut marker = pins.file("owner.json", false).map_err(io::Error::other)?;
            let mut bytes = Vec::new();
            (&mut marker).take(4097).read_to_end(&mut bytes)?;
            if bytes.len() > 4096 {
                return Err(invalid());
            }
            let saved: serde_json::Value = serde_json::from_slice(&bytes)?;
            let saved_id: FileIdentity = serde_json::from_value(saved["file"].clone())?;
            if saved["runId"] != run || saved["capacity"] != CAPACITY || saved_id != file_id {
                return Err(invalid());
            }
        } else {
            backing.set_len(CAPACITY)?;
            backing.sync_all()?;
            let mut marker = pins.file("owner.json", true).map_err(io::Error::other)?;
            marker.write_all(&serde_json::to_vec(
                &serde_json::json!({"runId":run,"capacity":CAPACITY,"file":file_id}),
            )?)?;
            marker.sync_all()?;
        }
        let cache = FileCache::from_owned_device_handles(file, SOURCE_BYTES, backing, CAPACITY)?;
        return Ok((cache, Some(pins)));
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .custom_flags(0xA0000000)
        .open(path)?;
    let mut length = [0u8; 8];
    let mut attrs = [0u8; 16];
    let mut device = [0u8; 12];
    if ioctl(&file, IOCTL_DISK_GET_LENGTH_INFO, &mut length)? != 8
        || u64::from_le_bytes(length) != SOURCE_BYTES
        || ioctl(&file, IOCTL_DISK_GET_DISK_ATTRIBUTES, &mut attrs)? != 16
        || u64::from_le_bytes(attrs[8..16].try_into().unwrap()) & 1 == 0
        || ioctl(&file, IOCTL_STORAGE_GET_DEVICE_NUMBER, &mut device)? != 12
        || u32::from_le_bytes(device[4..8].try_into().unwrap()) != number
    {
        return Err(io::Error::other(
            "Owned VHDX must be offline and retain its exact size/identity",
        ));
    }
    let path = root.join("ssd-handoff-cache.bin");
    let owner = root.join("device-cache-owner.json");
    if recovered {
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(owner)?)?;
        let backing = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .custom_flags(0xA0200000)
            .open(&path)?;
        let metadata = backing.metadata()?;
        if !metadata.is_file()
            || metadata.file_attributes() & 0x400 != 0
            || saved["runId"] != run
            || saved["created"] != metadata.creation_time()
            || saved["capacity"] != CAPACITY
        {
            return Err(invalid());
        }
        FileCache::from_owned_device_handles(file, SOURCE_BYTES, backing, CAPACITY)
            .map(|cache| (cache, None))
    } else {
        let cache = FileCache::create_from_owned_device(file, SOURCE_BYTES, &path, CAPACITY)?;
        let metadata = fs::metadata(path)?;
        let marker =
            serde_json::json!({"runId":run,"created":metadata.creation_time(),"capacity":CAPACITY});
        let mut record = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(owner)?;
        record.write_all(&serde_json::to_vec(&marker)?)?;
        record.sync_all()?;
        Ok((cache, None))
    }
}
