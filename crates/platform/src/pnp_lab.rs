//! Separate mounted-volume VM lab protocol. No desktop command installs or calls it.
use crate::driver::{LabAttachment, Telemetry, WireStatus};
use serde::Serialize;

pub const PROTOCOL_VERSION: u32 = 3;
#[repr(C)]
#[derive(Default)]
struct WireStatusV3 {
    cache: WireStatus,
    volume_count: u32,
    source_disk_number: u32,
    backing_disk_number: u32,
    pnp_state: u32,
    cancelled_reads: u64,
    child_cancellations: u64,
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
}
impl TryFrom<WireStatusV3> for Status {
    type Error = String;
    fn try_from(mut wire: WireStatusV3) -> Result<Self, Self::Error> {
        if wire.cache.version != PROTOCOL_VERSION || wire.pnp_state & !0x3f != 0 {
            return Err("Invalid mounted-volume lab status.".into());
        }
        wire.cache.version = crate::driver::PROTOCOL_VERSION;
        let mut cache: Telemetry = wire.cache.try_into()?;
        cache.protocol_version = PROTOCOL_VERSION;
        if cache.attached
            && (wire.volume_count == 0 || wire.source_disk_number == wire.backing_disk_number)
        {
            return Err("Mounted-volume lab disk identities are invalid.".into());
        }
        Ok(Self {
            cache,
            volume_count: wire.volume_count,
            source_disk_number: wire.source_disk_number,
            backing_disk_number: wire.backing_disk_number,
            pnp_state: wire.pnp_state,
            cancelled_reads: wire.cancelled_reads,
            child_cancellations: wire.child_cancellations,
        })
    }
}
fn encode(request: &LabAttachment) -> Result<crate::driver::WireAttach, String> {
    let mut wire = request.encode()?;
    // The v3 C side rejects trailing separators, ADS and ambiguous paths as well.
    if request.source_device.ends_with('\\')
        || request.backing_file.ends_with('\\')
        || request.source_device.chars().any(char::is_control)
        || request.backing_file.chars().any(char::is_control)
        || request
            .backing_file
            .char_indices()
            .any(|(i, ch)| ch == ':' && i != 5)
    {
        return Err("Ambiguous lab path or alternate data stream.".into());
    }
    wire.version = PROTOCOL_VERSION;
    Ok(wire)
}
#[cfg(windows)]
pub struct Controller(std::fs::File);
#[cfg(windows)]
impl Controller {
    pub fn open() -> Result<Self, String> {
        let controller = Self(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("\\\\.\\SuperHDDCachePnpLab")
                .map_err(|error| format!("Mounted-volume VM lab driver unavailable: {error}"))?,
        );
        if !controller.status()?.cache.experimental_attach_enabled {
            return Err("This build refuses mounted-volume cache configuration.".into());
        }
        Ok(controller)
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
        let mut wire = WireStatusV3::default();
        let output = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut wire as *mut WireStatusV3).cast::<u8>(),
                std::mem::size_of::<WireStatusV3>(),
            )
        };
        if self.ioctl(0x00226080, &[], output)? as usize != output.len() {
            return Err("Mounted-volume lab response size mismatch.".into());
        }
        wire.try_into()
    }
    pub fn configure(&self, request: &LabAttachment) -> Result<(), String> {
        let wire = encode(request)?;
        let input = unsafe {
            std::slice::from_raw_parts(
                (&wire as *const crate::driver::WireAttach).cast::<u8>(),
                std::mem::size_of::<crate::driver::WireAttach>(),
            )
        };
        self.ioctl(0x0022e084, input, &mut [])?;
        Ok(())
    }
    pub fn disable(&self) -> Result<(), String> {
        self.ioctl(0x0022e088, &[], &mut [])?;
        Ok(())
    }
    pub fn pause(&self, paused: bool) -> Result<(), String> {
        self.ioctl(0x0022e08c, &u32::from(paused).to_le_bytes(), &mut [])?;
        Ok(())
    }
    pub fn clear(&self) -> Result<(), String> {
        self.ioctl(0x0022e090, &[], &mut [])?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_abi_and_ioctl_match_c() {
        assert_eq!(std::mem::size_of::<WireStatusV3>(), 160);
        assert_eq!(std::mem::offset_of!(WireStatusV3, volume_count), 128);
        assert_eq!(std::mem::offset_of!(WireStatusV3, cancelled_reads), 144);
        assert_eq!(0x0022e084u32, (0x22 << 16) | (3 << 14) | (0x821 << 2));
    }
    #[test]
    fn protocol_downgrade_and_unknown_state_are_rejected() {
        assert!(Status::try_from(WireStatusV3::default()).is_err());
        let mut wire = WireStatusV3::default();
        wire.cache.version = PROTOCOL_VERSION;
        wire.pnp_state = 0x40;
        assert!(Status::try_from(wire).is_err());
    }
    #[test]
    fn configured_paths_match_kernel_guards() {
        let mut request = LabAttachment {
            source_device: "\\Device\\HarddiskVolume42".into(),
            backing_file: "\\??\\V:\\cache\\payload.bin".into(),
            capacity_bytes: 1024 * 1024,
            acknowledged_disposable_vhds: true,
        };
        assert_eq!(encode(&request).unwrap().version, 3);
        request.backing_file.push_str(":stream");
        assert!(encode(&request).is_err());
        request.backing_file = "\\??\\V:\\cache\\payload.bin".into();
        request.source_device.push('\\');
        assert!(encode(&request).is_err());
        request.source_device.pop();
        request.acknowledged_disposable_vhds = false;
        assert!(encode(&request).is_err());
    }
}
