//! v4 native volume protocol. Desktop/service access is read-only; attachment
//! and other mutations remain isolated-lab developer operations.
use crate::driver::{NativeVolumeTelemetry, Telemetry, WireStatus, MAX_CAPACITY_BYTES};
use serde::{Deserialize, Serialize};
pub const PROTOCOL_VERSION: u32 = 4;
#[repr(C)]
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiagnostic {
    pub stage: u32,
    pub status: u32,
    pub source_file_flags: u32,
    pub source_device_type: u32,
    pub lower_flags: u32,
    pub pnp_state: u32,
    pub cache_file_flags: u32,
    pub cache_device_type: u32,
    pub active_mutations: u32,
    pub pending_controls: u32,
    pub last_control_major: u32,
    pub last_control_code: u32,
}
#[repr(C)]
#[derive(Default)]
struct Wire {
    cache: WireStatus,
    volume_count: u32,
    source_disk_number: u32,
    backing_disk_number: u32,
    pnp_state: u32,
    cancelled_reads: u64,
    child_cancellations: u64,
    raw_backend: u32,
    store_locked: u32,
    allocated_bytes: u64,
    imported_records: u64,
    revalidated_records: u64,
    validation_read_bytes: u64,
    paging_hits: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub cache: Telemetry,
    pub volume_count: u32,
    pub source_disk_number: u32,
    pub backing_disk_number: u32,
    pub pnp_state: u32,
    pub cancelled_reads: u64,
    pub child_cancellations: u64,
    pub raw_backend: bool,
    pub store_locked: bool,
    pub allocated_bytes: u64,
    pub imported_records: u64,
    pub revalidated_records: u64,
    pub validation_read_bytes: u64,
    pub paging_hits: u64,
}
impl TryFrom<Wire> for Status {
    type Error = String;
    fn try_from(mut wire: Wire) -> Result<Self, String> {
        if wire.cache.version != PROTOCOL_VERSION
            || wire.pnp_state & !0x3f != 0
            || wire.raw_backend > 1
            || wire.store_locked > 1
        {
            return Err("Invalid RAW lab response flags or version".into());
        }
        wire.cache.version = crate::driver::PROTOCOL_VERSION;
        let mut cache: Telemetry = wire.cache.try_into()?;
        cache.protocol_version = PROTOCOL_VERSION;
        if cache.attached
            && (wire.volume_count == 0
                || wire.source_disk_number == wire.backing_disk_number
                || wire.raw_backend != 1
                || wire.store_locked != 1
                || wire.allocated_bytes <= cache.capacity_bytes
                || wire.allocated_bytes > MAX_CAPACITY_BYTES
                || !wire.allocated_bytes.is_multiple_of(4096)
                || wire.imported_records > cache.capacity_bytes / 65536)
        {
            return Err("Invalid RAW lab identities, lock, or capacity".into());
        }
        cache.native_volume = Some(NativeVolumeTelemetry {
            volume_count: wire.volume_count,
            source_disk_number: cache.attached.then_some(wire.source_disk_number),
            backing_disk_number: cache.attached.then_some(wire.backing_disk_number),
            pnp_state: wire.pnp_state,
            store_locked: wire.store_locked != 0,
            allocated_bytes: wire.allocated_bytes,
            cancelled_reads: wire.cancelled_reads,
            child_cancellations: wire.child_cancellations,
            paging_hits: wire.paging_hits,
            revalidated_records: wire.revalidated_records,
            validation_read_bytes: wire.validation_read_bytes,
        });
        Ok(Self {
            cache,
            volume_count: wire.volume_count,
            source_disk_number: wire.source_disk_number,
            backing_disk_number: wire.backing_disk_number,
            pnp_state: wire.pnp_state,
            cancelled_reads: wire.cancelled_reads,
            child_cancellations: wire.child_cancellations,
            raw_backend: wire.raw_backend != 0,
            store_locked: wire.store_locked != 0,
            allocated_bytes: wire.allocated_bytes,
            imported_records: wire.imported_records,
            revalidated_records: wire.revalidated_records,
            validation_read_bytes: wire.validation_read_bytes,
            paging_hits: wire.paging_hits,
        })
    }
}
/// Read-only desktop/service observation. Only a missing device permits legacy
/// fallback; access denial, corrupt responses and version mismatch are surfaced.
#[cfg(windows)]
pub fn try_telemetry() -> Result<Option<Status>, String> {
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .open("\\\\.\\SuperHDDCacheRawLab")
    {
        Ok(file) => file,
        Err(error) if matches!(error.raw_os_error(), Some(2 | 3)) => return Ok(None),
        Err(error) => return Err(format!("Native v4 telemetry unavailable: {error}")),
    };
    read_status(&file).map(Some)
}
#[cfg(windows)]
fn read_status(file: &std::fs::File) -> Result<Status, String> {
    use std::os::windows::io::AsRawHandle;
    let mut wire = Wire::default();
    let size = std::mem::size_of::<Wire>();
    let mut returned = 0;
    let ok = unsafe {
        windows_sys::Win32::System::IO::DeviceIoControl(
            file.as_raw_handle(),
            0x00226080,
            std::ptr::null(),
            0,
            (&mut wire as *mut Wire).cast(),
            size as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    if returned as usize != size {
        return Err("RAW status size mismatch".into());
    }
    wire.try_into()
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub source_device: String,
    pub cache_device: String,
    pub capacity_bytes: u64,
    pub acknowledged_disposable_vhds: bool,
}
#[repr(C)]
struct AttachWire {
    version: u32,
    ack: u32,
    capacity: u64,
    source: [u16; 260],
    backing: [u16; 520],
}
fn path<const N: usize>(text: &str) -> Result<[u16; N], String> {
    let suffix = text
        .strip_prefix("\\Device\\HarddiskVolume")
        .ok_or("Use a volume device path")?;
    let units: Vec<_> = text.encode_utf16().collect();
    if suffix.is_empty() || !suffix.bytes().all(|ch| ch.is_ascii_digit()) || units.len() >= N {
        return Err("Invalid volume device path".into());
    }
    let mut result = [0; N];
    result[..units.len()].copy_from_slice(&units);
    Ok(result)
}
impl Attachment {
    fn encode(&self) -> Result<AttachWire, String> {
        if !self.acknowledged_disposable_vhds
            || self.capacity_bytes < 131072
            || self.capacity_bytes > MAX_CAPACITY_BYTES
            || !self.capacity_bytes.is_multiple_of(65536)
            || self.source_device == self.cache_device
        {
            return Err("Require distinct disposable VHD volumes and aligned total cache budget below 25 decimal GB".into());
        }
        Ok(AttachWire {
            version: 4,
            ack: 0x54455354,
            capacity: self.capacity_bytes,
            source: path(&self.source_device)?,
            backing: path(&self.cache_device)?,
        })
    }
}
/// Produces a marker only; the caller must not write it to an existing user disk.
pub fn bootstrap(instance: [u8; 16]) -> Result<[u8; 4096], String> {
    superhdd_storage_control::format::bootstrap(instance)
}
#[cfg(windows)]
pub struct Controller(std::fs::File);
#[cfg(windows)]
impl Controller {
    pub fn open() -> Result<Self, String> {
        let device = Self(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("\\\\.\\SuperHDDCacheRawLab")
                .map_err(|e| format!("RAW lab driver unavailable: {e}"))?,
        );
        if !device.status()?.cache.experimental_attach_enabled {
            return Err("This driver refuses RAW lab configuration".into());
        }
        Ok(device)
    }
    fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<u32, String> {
        use std::os::windows::io::AsRawHandle;
        let mut returned = 0;
        let ok = unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
                self.0.as_raw_handle(),
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(returned)
    }
    pub fn status(&self) -> Result<Status, String> {
        read_status(&self.0)
    }
    pub fn configure(&self, request: &Attachment) -> Result<(), String> {
        let wire = request.encode()?;
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&wire as *const AttachWire).cast::<u8>(),
                std::mem::size_of::<AttachWire>(),
            )
        };
        if let Err(error) = self.ioctl(0x0022e084, bytes, &mut []) {
            return Err(match self.diagnostic() {
                Ok(detail) => format!("{error}; configuration diagnostic: {detail:?}"),
                Err(_) => error,
            });
        }
        Ok(())
    }
    pub fn diagnostic(&self) -> Result<ConfigurationDiagnostic, String> {
        let mut diagnostic = ConfigurationDiagnostic::default();
        let size = std::mem::size_of::<ConfigurationDiagnostic>();
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut diagnostic as *mut ConfigurationDiagnostic).cast::<u8>(),
                size,
            )
        };
        if self.ioctl(0x00226094, &[], bytes)? as usize != size {
            return Err("Configuration diagnostic size mismatch".into());
        }
        Ok(diagnostic)
    }
    pub fn pause(&self, paused: bool) -> Result<(), String> {
        self.ioctl(0x0022e08c, &u32::from(paused).to_le_bytes(), &mut [])?;
        Ok(())
    }
    pub fn clear(&self) -> Result<(), String> {
        self.ioctl(0x0022e090, &[], &mut [])?;
        Ok(())
    }
    pub fn disable(&self) -> Result<(), String> {
        self.ioctl(0x0022e088, &[], &mut [])?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_matches_kernel() {
        assert_eq!(std::mem::size_of::<Wire>(), 208);
        assert_eq!(std::mem::size_of::<AttachWire>(), 1576);
        assert_eq!(std::mem::size_of::<ConfigurationDiagnostic>(), 48);
        assert_eq!(std::mem::offset_of!(Wire, allocated_bytes), 168);
    }
    #[test]
    fn refuses_unowned_or_ambiguous_volumes() {
        let mut request = Attachment {
            source_device: "\\Device\\HarddiskVolume41".into(),
            cache_device: "\\Device\\HarddiskVolume42".into(),
            capacity_bytes: 1048576,
            acknowledged_disposable_vhds: true,
        };
        assert!(request.encode().is_ok());
        request.cache_device.push_str(":stream");
        assert!(request.encode().is_err());
        request.cache_device = request.source_device.clone();
        assert!(request.encode().is_err());
        request.cache_device = "\\Device\\HarddiskVolume42".into();
        request.acknowledged_disposable_vhds = false;
        assert!(request.encode().is_err());
    }
    #[test]
    fn marker_wire_format() {
        assert!(bootstrap([0; 16]).is_err());
        let marker = bootstrap([0x31; 16]).unwrap();
        assert_eq!(&marker[..8], b"SHDDRAW4");
        assert_eq!(u32::from_le_bytes(marker[52..56].try_into().unwrap()), 1);
        assert!(marker[128..].iter().all(|byte| *byte == 0));
    }
    #[test]
    fn rejects_protocol_confusion() {
        assert!(Status::try_from(Wire::default()).is_err());
    }
    #[test]
    fn validates_external_attached_status_bytes() {
        let mut bytes = [0u8; 208];
        for (offset, value) in [
            (0, 4u32),
            (4, 1),
            (12, 1),
            (120, 16),
            (124, 1048576),
            (128, 1),
            (132, 2),
            (136, 3),
            (140, 1),
            (160, 1),
            (164, 1),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (72, 1u64),
            (80, 65536),
            (88, 1048576),
            (168, 73728),
            (176, 1),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        let decode =
            |data: &[u8; 208]| unsafe { std::ptr::read_unaligned(data.as_ptr().cast::<Wire>()) };
        let status = Status::try_from(decode(&bytes)).unwrap();
        let native = status.cache.native_volume.as_ref().unwrap();
        assert_eq!(status.cache.protocol_version, 4);
        assert_eq!(native.source_disk_number, Some(2));
        assert_eq!(native.backing_disk_number, Some(3));
        assert!(native.store_locked);
        assert_eq!(native.allocated_bytes, 73728);
        let desktop_json = serde_json::to_value(&status.cache).unwrap();
        assert_eq!(desktop_json["nativeVolume"]["sourceDiskNumber"], 2);
        assert_eq!(desktop_json["protocolVersion"], 4);
        for (offset, value) in [(0, 3u32), (164, 0), (160, 2), (136, 2)] {
            let mut bad = bytes;
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Status::try_from(decode(&bad)).is_err());
        }
        let mut bad = bytes;
        bad[176..184].copy_from_slice(&2u64.to_le_bytes());
        assert!(Status::try_from(decode(&bad)).is_err());
        bad = bytes;
        bad[168..176].copy_from_slice(&65536u64.to_le_bytes());
        assert!(Status::try_from(decode(&bad)).is_err());
    }
}
