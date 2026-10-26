use super::*;
use crate::lab::LabIdentity;
use std::io::Read;
use windows_sys::Win32::System::{
    SystemInformation::{GetSystemFirmwareTable, RSMB},
    WindowsProgramming::GetComputerNameW,
};

#[derive(Clone)]
pub struct VerifiedLab {
    identity: LabIdentity,
}
fn bios() -> Result<(String, String)> {
    // Gen1 Hyper-V does not always populate the optional BIOS registry key.
    // Read the documented SMBIOS API rather than weakening hardware identity.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, null_mut(), 0) };
    if !(8..=1024 * 1024).contains(&size) {
        return Err(error("Read VM firmware size", unsafe { GetLastError() }));
    }
    let mut buffer = vec![0u8; size as usize];
    if unsafe { GetSystemFirmwareTable(RSMB, 0, buffer.as_mut_ptr(), size) } != size {
        return Err("VM firmware table changed or could not be read.".into());
    }
    crate::lab::smbios_identity(&buffer)
}
impl VerifiedLab {
    /// Firmware, native computer name and the fixed provisioned VM marker are
    /// checked before any file/volume request from the caller is acted upon.
    pub fn verify(identity: LabIdentity) -> Result<Self> {
        identity.validate()?;
        let mut computer = [0u16; 64];
        let mut length = computer.len() as u32;
        if unsafe { GetComputerNameW(computer.as_mut_ptr(), &mut length) } == 0
            || length as usize >= computer.len()
            || String::from_utf16(&computer[..length as usize]).map_err(|e| e.to_string())?
                != "SHDD-LAB"
            || bios()? != ("Microsoft Corporation".into(), "Virtual Machine".into())
        {
            return Err(
                "Isolated lab storage refused outside the provisioned Super HDD VM.".into(),
            );
        }
        let mut marker = String::new();
        File::open("C:\\SuperHDDLab\\vm-identity.txt")
            .map_err(|e| e.to_string())?
            .take(129)
            .read_to_string(&mut marker)
            .map_err(|e| e.to_string())?;
        if !marker.trim().eq_ignore_ascii_case(&identity.vm_id) {
            return Err("Isolated VM marker does not match the requested VM.".into());
        }
        Ok(Self { identity })
    }
    pub fn identity(&self) -> &LabIdentity {
        &self.identity
    }
    pub fn validate_root(&self, root: &Path) -> Result<()> {
        if root.file_name().and_then(|s| s.to_str())
            != Some(&format!("managed-{}", self.identity.run_id))
        {
            return Err("Use the new run-owned managed-<run-id> container directory.".into());
        }
        Ok(())
    }
    pub fn validate_parent(&self, parent: &VolumeIdentity) -> Result<()> {
        self.check_volume(parent, &self.identity.cache_parent_volume, "SHDD-M-CACHE-")
    }
    pub fn validate_source_parent(&self, parent: &VolumeIdentity) -> Result<()> {
        self.check_volume(
            parent,
            &self.identity.source_parent_volume,
            "SHDD-M-SOURCE-",
        )
    }
    pub fn validate_source_backing(
        &self,
        parent: &VolumeIdentity,
        dependencies: &[Dependency],
    ) -> Result<()> {
        self.validate_source_parent(parent)?;
        let [dependency] = dependencies else {
            return Err("Missing or nested lab source dependency.".into());
        };
        // DiskPart's run-owned NTFS source persists after the setup process exits.
        // Only this source may have PERMANENT_LIFETIME (1024); owned RAW cache
        // attachments still require the service's non-permanent handle lease.
        if !dependency.microsoft_provider
            || dependency.device_id != 3
            || dependency.provider_flags != 0
            || dependency.ancestor_level != 0
            || !same_volume(&dependency.host_volume, &parent.volume_guid)
            || dependency.flags & !(2 | 64 | 128 | 1024) != 0
            || dependency.flags & 2 == 0
            || !dependency
                .relative_path
                .trim_start_matches('\\')
                .eq_ignore_ascii_case(&format!("source-{}.vhdx", self.identity.run_id))
        {
            return Err(format!(
                "Unproven fully allocated run-owned lab source: {dependency:?}"
            ));
        }
        Ok(())
    }
    fn check_volume(&self, parent: &VolumeIdentity, expected: &str, label: &str) -> Result<()> {
        if !same_volume(&parent.volume_guid, expected)
            || parent.removable
            || !matches!(parent.bus_type, 1 | 3 | 10 | 11 | 14 | 15)
            || parent.extent_length < 512 * 1024 * 1024
            || parent.extent_length > 4 * 1024 * 1024 * 1024
        {
            return Err(
                "Isolated lab parent is not the bounded run-owned virtual data volume.".into(),
            );
        }
        let guid = wide(&parent.volume_guid)?;
        let mut actual = [0u16; 64];
        if unsafe {
            GetVolumeInformationW(
                guid.as_ptr(),
                actual.as_mut_ptr(),
                actual.len() as u32,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
            )
        } == 0
        {
            return Err(error("Verify lab parent label", unsafe { GetLastError() }));
        }
        let actual = String::from_utf16(
            &actual[..actual
                .iter()
                .position(|c| *c == 0)
                .ok_or("Invalid lab label")?],
        )
        .map_err(|e| e.to_string())?;
        if actual != format!("{label}{}", &self.identity.run_id[..8])
            || volume_filesystem(&parent.volume_guid)? != "NTFS"
        {
            return Err(
                "Isolated lab parent label/filesystem differs from the run fixture.".into(),
            );
        }
        Ok(())
    }
    pub fn validate_pair(&self, source: &VolumeIdentity, cache: &VolumeIdentity) -> Result<()> {
        self.validate_source_parent(source)?;
        self.validate_parent(cache)?;
        if source.disk_number == cache.disk_number
            || same_volume(&source.volume_guid, &cache.volume_guid)
        {
            return Err("Source/cache lab parents overlap.".into());
        }
        Ok(())
    }
    pub(crate) fn validate_backing(
        &self,
        parent: &VolumeIdentity,
        dependencies: &[Dependency],
        persistent: bool,
    ) -> Result<()> {
        self.validate_parent(parent)?;
        let [dependency] = dependencies else {
            return Err("Missing or nested lab backing dependency.".into());
        };
        if !dependency.microsoft_provider
            || dependency.device_id != 3
            || dependency.provider_flags != 0
            || dependency.ancestor_level != 0
            || !same_volume(&dependency.host_volume, &parent.volume_guid)
            || dependency.flags & !(2 | 64 | 128 | if persistent { 1024 } else { 0 }) != 0
            || dependency.flags & 2 == 0
            || (dependency.flags & 1024 != 0) != persistent
        {
            return Err("Lab VHDX is not exclusively backed by its run-owned parent.".into());
        }
        Ok(())
    }
}
