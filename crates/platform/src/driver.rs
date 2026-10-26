use serde::{Deserialize, Serialize};
pub const PROTOCOL_VERSION: u32 = 2;
pub const MAX_CAPACITY_BYTES: u64 = (25_000_000_000 / 65536) * 65536;
#[repr(C)]
#[derive(Default)]
pub(super) struct WireStatus {
    pub(super) version: u32,
    attached: u32,
    paused: u32,
    experimental_attach_enabled: u32,
    hits: u64,
    misses: u64,
    source_read_bytes: u64,
    ssd_read_bytes: u64,
    fill_bytes: u64,
    evictions: u64,
    errors: u64,
    generation: u64,
    capacity_bytes: u64,
    source_length: u64,
    active_mutations: u32,
    pending_reads: u32,
    cache_disabled: u32,
    reserved: u32,
    read_bypasses: u64,
    queue_limit: u32,
    max_read_bytes: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVolumeTelemetry {
    pub volume_count: u32,
    pub source_disk_number: Option<u32>,
    pub backing_disk_number: Option<u32>,
    pub pnp_state: u32,
    pub store_locked: bool,
    pub allocated_bytes: u64,
    pub cancelled_reads: u64,
    pub child_cancellations: u64,
    pub paging_hits: u64,
    pub revalidated_records: u64,
    pub validation_read_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Telemetry {
    pub protocol_version: u32,
    pub experimental_attach_enabled: bool,
    pub attached: bool,
    pub paused: bool,
    pub hits: u64,
    pub misses: u64,
    pub source_read_bytes: u64,
    pub ssd_read_bytes: u64,
    pub fill_bytes: u64,
    pub evictions: u64,
    pub errors: u64,
    pub generation: u64,
    pub capacity_bytes: u64,
    pub source_length: u64,
    pub active_mutations: u32,
    pub pending_reads: u32,
    pub cache_disabled: bool,
    pub read_bypasses: u64,
    pub queue_limit: u32,
    pub max_read_bytes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_volume: Option<NativeVolumeTelemetry>,
}
impl TryFrom<WireStatus> for Telemetry {
    type Error = String;
    fn try_from(wire: WireStatus) -> Result<Self, Self::Error> {
        if wire.version != PROTOCOL_VERSION
            || wire.reserved != 0
            || [
                wire.attached,
                wire.paused,
                wire.experimental_attach_enabled,
                wire.cache_disabled,
            ]
            .into_iter()
            .any(|flag| flag > 1)
            || wire.queue_limit != 16
            || wire.max_read_bytes != 1024 * 1024
            || wire.pending_reads > wire.queue_limit
            || (wire.attached == 1
                && (wire.capacity_bytes < 65536
                    || wire.capacity_bytes > MAX_CAPACITY_BYTES
                    || !wire.capacity_bytes.is_multiple_of(65536)
                    || wire.source_length < 65536
                    || wire.generation == 0))
        {
            return Err("Invalid driver v2 status or unsupported protocol.".into());
        }
        Ok(Self {
            protocol_version: wire.version,
            experimental_attach_enabled: wire.experimental_attach_enabled != 0,
            attached: wire.attached != 0,
            paused: wire.paused != 0,
            hits: wire.hits,
            misses: wire.misses,
            source_read_bytes: wire.source_read_bytes,
            ssd_read_bytes: wire.ssd_read_bytes,
            fill_bytes: wire.fill_bytes,
            evictions: wire.evictions,
            errors: wire.errors,
            generation: wire.generation,
            capacity_bytes: wire.capacity_bytes,
            source_length: wire.source_length,
            active_mutations: wire.active_mutations,
            pending_reads: wire.pending_reads,
            cache_disabled: wire.cache_disabled != 0,
            read_bypasses: wire.read_bypasses,
            queue_limit: wire.queue_limit,
            max_read_bytes: wire.max_read_bytes,
            native_volume: None,
        })
    }
}
pub fn telemetry() -> Result<Telemetry, String> {
    #[cfg(not(windows))]
    {
        return Err("The cache driver requires Windows.".into());
    }
    #[cfg(windows)]
    {
        // Both endpoints belong to Super HDD. Prefer the current mounted-volume
        // implementation; an invalid/inaccessible v4 reply must not be hidden
        // behind unrelated legacy counters. Neither path loads a driver.
        if let Some(status) = crate::paging_driver::try_telemetry()? {
            return Ok(status.cache);
        }
        if let Some(status) = crate::transparent_driver::try_telemetry()? {
            return Ok(status.cache);
        }
        if let Some(status) = crate::direct_driver::try_telemetry()? {
            return Ok(status.cache);
        }
        if let Some(status) = crate::raw_lab::try_telemetry()? {
            return Ok(status.cache);
        }
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::IO::DeviceIoControl;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open("\\\\.\\SuperHDDCacheDriver")
            .map_err(|e| format!("Driver is unavailable or access is denied: {e}"))?;
        let mut wire = WireStatus::default();
        let mut returned = 0;
        unsafe {
            if DeviceIoControl(
                file.as_raw_handle(),
                0x00226000,
                std::ptr::null(),
                0,
                (&mut wire as *mut WireStatus).cast(),
                std::mem::size_of::<WireStatus>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        if returned as usize != std::mem::size_of::<WireStatus>() {
            return Err("Driver protocol version or response size does not match.".into());
        }
        wire.try_into()
    }
}

/// Developer-only controller for a separately signed isolated-VM lab build.
/// It is deliberately not exposed through the desktop or interactive-user service pipe.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabAttachment {
    pub source_device: String,
    pub backing_file: String,
    pub capacity_bytes: u64,
    pub acknowledged_disposable_vhds: bool,
}
#[repr(C)]
pub(super) struct WireAttach {
    pub(super) version: u32,
    acknowledgement: u32,
    capacity: u64,
    source: [u16; 260],
    backing: [u16; 520],
}
fn encode_name<const N: usize>(text: &str) -> Result<[u16; N], String> {
    let units: Vec<_> = text.encode_utf16().collect();
    if units.is_empty() || units.len() >= N || units.contains(&0) {
        return Err("Invalid or oversized driver path.".into());
    }
    let mut result = [0; N];
    result[..units.len()].copy_from_slice(&units);
    Ok(result)
}
impl LabAttachment {
    pub(super) fn encode(&self) -> Result<WireAttach, String> {
        if !self.acknowledged_disposable_vhds
            || self.capacity_bytes < 65536
            || self.capacity_bytes > MAX_CAPACITY_BYTES
            || !self.capacity_bytes.is_multiple_of(65536)
            || !self.source_device.starts_with("\\Device\\")
            || self.source_device.contains(['/', ':'])
            || self.source_device.contains("..")
            || !self.backing_file.starts_with("\\??\\")
            || self.backing_file.contains('/')
            || self.backing_file.contains("..")
        {
            return Err("Use acknowledged disposable VHDs, absolute NT paths and an aligned capacity below 25 decimal GB.".into());
        }
        Ok(WireAttach {
            version: PROTOCOL_VERSION,
            acknowledgement: 0x54455354,
            capacity: self.capacity_bytes,
            source: encode_name(&self.source_device)?,
            backing: encode_name(&self.backing_file)?,
        })
    }
}
#[cfg(windows)]
pub struct LabDriver(std::fs::File);
#[cfg(windows)]
impl LabDriver {
    pub fn open() -> Result<Self, String> {
        let controller = Self(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("\\\\.\\SuperHDDCacheDriver")
                .map_err(|error| format!("Lab driver unavailable or access denied: {error}"))?,
        );
        let status = controller.status()?;
        if !status.experimental_attach_enabled {
            return Err("This driver rejects experimental attachment.".into());
        }
        Ok(controller)
    }
    fn ioctl(
        &self,
        code: u32,
        input: Option<&[u8]>,
        output: Option<&mut [u8]>,
    ) -> Result<u32, String> {
        use std::os::windows::io::AsRawHandle;
        let input = input.unwrap_or(&[]);
        let output = output.unwrap_or(&mut []);
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
    pub fn status(&self) -> Result<Telemetry, String> {
        let mut wire = WireStatus::default();
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut wire as *mut WireStatus).cast::<u8>(),
                std::mem::size_of::<WireStatus>(),
            )
        };
        if self.ioctl(0x00226000, None, Some(output))? as usize != std::mem::size_of::<WireStatus>()
        {
            return Err("Driver status size mismatch.".into());
        }
        wire.try_into()
    }
    pub fn attach(&self, request: &LabAttachment) -> Result<(), String> {
        let wire = request.encode()?;
        let input = unsafe {
            std::slice::from_raw_parts(
                (&wire as *const WireAttach).cast::<u8>(),
                std::mem::size_of::<WireAttach>(),
            )
        };
        self.ioctl(0x0022e004, Some(input), None)?;
        Ok(())
    }
    pub fn pause(&self, paused: bool) -> Result<(), String> {
        self.ioctl(0x0022e00c, Some(&u32::from(paused).to_le_bytes()), None)?;
        Ok(())
    }
    pub fn clear(&self) -> Result<(), String> {
        self.ioctl(0x0022e010, None, None)?;
        Ok(())
    }
    /// Caller must quiesce the isolated raw test source before detaching.
    pub fn detach(&self) -> Result<(), String> {
        self.ioctl(0x0022e008, None, None)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernel_status_abi_matches() {
        assert_eq!(std::mem::size_of::<WireStatus>(), 128);
        assert_eq!(std::mem::offset_of!(WireStatus, capacity_bytes), 80);
        assert_eq!(std::mem::offset_of!(WireStatus, read_bypasses), 112);
        assert_eq!(std::mem::size_of::<WireAttach>(), 1576);
    }
    fn status() -> WireStatus {
        WireStatus {
            version: PROTOCOL_VERSION,
            queue_limit: 16,
            max_read_bytes: 1024 * 1024,
            ..Default::default()
        }
    }
    #[test]
    fn invalid_protocol_flags_and_queue_lengths_are_rejected() {
        for data in [
            WireStatus {
                version: 1,
                ..status()
            },
            WireStatus {
                cache_disabled: 2,
                ..status()
            },
            WireStatus {
                reserved: 1,
                ..status()
            },
            WireStatus {
                pending_reads: 17,
                ..status()
            },
            WireStatus {
                attached: 1,
                ..status()
            },
        ] {
            assert!(Telemetry::try_from(data).is_err());
        }
    }
    #[test]
    fn lab_bypass_and_pause_status_are_preserved_without_claiming_qualification() {
        let data = Telemetry::try_from(WireStatus {
            attached: 1,
            paused: 1,
            experimental_attach_enabled: 1,
            cache_disabled: 1,
            capacity_bytes: 65536,
            source_length: 131072,
            generation: 2,
            ..status()
        })
        .unwrap();
        assert!(data.paused && data.cache_disabled && data.experimental_attach_enabled);
        assert_eq!(data.capacity_bytes, 65536);
    }
    fn request() -> LabAttachment {
        LabAttachment {
            source_device: "\\Device\\HarddiskVolume9".into(),
            backing_file: "\\??\\Z:\\lab\\cache.shdd-cache".into(),
            capacity_bytes: 65536,
            acknowledged_disposable_vhds: true,
        }
    }
    #[test]
    fn lab_attach_paths_and_capacity_are_bounded() {
        assert!(request().encode().is_ok());
        for capacity in [0, 65537, MAX_CAPACITY_BYTES + 65536] {
            let mut data = request();
            data.capacity_bytes = capacity;
            assert!(data.encode().is_err());
        }
        let mut data = request();
        data.acknowledged_disposable_vhds = false;
        assert!(data.encode().is_err());
        for path in ["C:\\data", "\\Device\\a\0b", "\\Device\\..\\a"] {
            let mut data = request();
            data.source_device = path.into();
            assert!(data.encode().is_err());
        }
        assert!(encode_name::<4>("ab😀").is_err());
        assert!(encode_name::<5>("ab😀").is_ok());
    }
}
