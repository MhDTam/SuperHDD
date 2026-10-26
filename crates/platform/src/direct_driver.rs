//! Native v5 direct-data-volume protocol, distinct from the frozen v4 lab ABI.
use crate::driver::{NativeVolumeTelemetry, Telemetry, WireStatus, MAX_CAPACITY_BYTES};
use serde::Serialize;
pub const PROTOCOL_VERSION: u32 = 5;
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
    direct_vm_build: u32,
    policy_version: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub vm_test_build: bool,
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
        if wire.direct_vm_build > 1
            || wire.policy_version != 1
            || wire.cache.version != PROTOCOL_VERSION
            || wire.pnp_state & !0x3f != 0
            || wire.raw_backend > 1
            || wire.store_locked > 1
        {
            return Err("Invalid Native v5 response flags or version".into());
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
            return Err("Invalid Native v5 identities, lock, or capacity".into());
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
            vm_test_build: wire.direct_vm_build != 0,
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
        .open("\\\\.\\SuperHDDCacheV5")
    {
        Ok(file) => file,
        Err(error) if matches!(error.raw_os_error(), Some(2 | 3)) => return Ok(None),
        Err(error) => return Err(format!("Native v5 telemetry unavailable: {error}")),
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
#[derive(Debug)]
pub struct Attachment {
    pub source_device: String,
    pub cache_device: String,
    pub cache_parent_device: String,
    pub capacity_bytes: u64,
    pub source_disk: u32,
    pub cache_parent_disk: u32,
    pub source_offset: u64,
    pub source_bytes: u64,
    pub vm_test: bool,
}
#[repr(C)]
struct AttachWire {
    version: u32,
    ack: u32,
    capacity: u64,
    source: [u16; 260],
    backing: [u16; 520],
    parent: [u16; 260],
    source_disk: u32,
    parent_disk: u32,
    source_offset: u64,
    source_bytes: u64,
}
fn path<const N: usize>(value: &str) -> Result<[u16; N], String> {
    let suffix = value
        .strip_prefix(r"\Device\HarddiskVolume")
        .ok_or("Use an exact native volume device")?;
    let units: Vec<_> = value.encode_utf16().collect();
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) || units.len() >= N {
        return Err("Invalid native volume device".into());
    }
    let mut result = [0; N];
    result[..units.len()].copy_from_slice(&units);
    Ok(result)
}
impl Attachment {
    fn encode(&self) -> Result<AttachWire, String> {
        if self.capacity_bytes < 131072
            || self.capacity_bytes > MAX_CAPACITY_BYTES
            || !self.capacity_bytes.is_multiple_of(65536)
            || self.source_disk == self.cache_parent_disk
            || self.source_device == self.cache_device
            || self.source_device == self.cache_parent_device
            || self.cache_device == self.cache_parent_device
            || self.source_offset == 0
            || self.source_bytes < 65536
            || self
                .source_offset
                .checked_add(self.source_bytes)
                .is_none_or(|n| n > i64::MAX as u64)
        {
            return Err("Invalid direct-source topology, bounds or cache budget".into());
        }
        Ok(AttachWire {
            version: 5,
            ack: if self.vm_test { 0x564d5445 } else { 0x44415441 },
            capacity: self.capacity_bytes,
            source: path(&self.source_device)?,
            backing: path(&self.cache_device)?,
            parent: path(&self.cache_parent_device)?,
            source_disk: self.source_disk,
            parent_disk: self.cache_parent_disk,
            source_offset: self.source_offset,
            source_bytes: self.source_bytes,
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
    pub fn open(vm_test: bool) -> Result<Self, String> {
        let device = Self(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("\\\\.\\SuperHDDCacheV5")
                .map_err(|e| format!("Native v5 driver unavailable: {e}"))?,
        );
        if !device.status()?.cache.experimental_attach_enabled
            || device.status()?.vm_test_build != vm_test
        {
            return Err("This driver refuses Native v5 configuration".into());
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
        if self.status()?.vm_test_build != request.vm_test {
            return Err("Driver policy differs from request".into());
        }
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
    fn request() -> Attachment {
        Attachment {
            source_device: r"\Device\HarddiskVolume1".into(),
            cache_device: r"\Device\HarddiskVolume2".into(),
            cache_parent_device: r"\Device\HarddiskVolume3".into(),
            capacity_bytes: 1048576,
            source_disk: 1,
            cache_parent_disk: 2,
            source_offset: 1048576,
            source_bytes: 1073741824,
            vm_test: false,
        }
    }
    #[test]
    fn v5_abi_and_parent_identity_match_the_kernel() {
        assert_eq!(std::mem::size_of::<Wire>(), 216);
        assert_eq!(std::mem::size_of::<AttachWire>(), 2120);
        assert_eq!(std::mem::offset_of!(AttachWire, source_offset), 2104);
        let physical = request().encode().unwrap();
        assert_eq!(physical.ack, 0x44415441);
        let mut vm = request();
        vm.vm_test = true;
        assert_eq!(vm.encode().unwrap().ack, 0x564d5445);
    }
    #[test]
    fn direct_configuration_refuses_recursion_aliases_and_overflow() {
        let mut r = request();
        r.cache_parent_disk = r.source_disk;
        assert!(r.encode().is_err());
        let mut r = request();
        r.cache_parent_device = r.source_device.clone();
        assert!(r.encode().is_err());
        let mut r = request();
        r.source_offset = u64::MAX;
        assert!(r.encode().is_err());
        let mut r = request();
        r.cache_device.push_str(":stream");
        assert!(r.encode().is_err());
    }
    #[test]
    fn unknown_source_policy_flags_are_not_accepted() {
        let w = Wire {
            direct_vm_build: 2,
            policy_version: 1,
            ..Default::default()
        };
        assert!(Status::try_from(w).is_err());
        let w = Wire {
            direct_vm_build: 0,
            policy_version: 2,
            ..Default::default()
        };
        assert!(Status::try_from(w).is_err());
    }
}
