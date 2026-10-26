//! Native storage-container control for the production implementation in progress.
//! This crate does not install a driver, attach a source filter, or relax a kernel gate.
use serde::{Deserialize, Serialize};
pub mod format;
pub mod journal;
pub mod lab;
pub mod recovery;
pub mod session;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub mod iscsi;
#[cfg(windows)]
pub mod data_source;
#[cfg(windows)]
pub mod loopback;
#[cfg(windows)]
pub mod port_reservation;

pub const MAX_BUDGET_BYTES: u64 = 25_000_000_000;
const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerBudget {
    pub total_bytes: u64,
    pub virtual_disk_bytes: u64,
    pub reserved_container_overhead_bytes: u64,
}
impl ContainerBudget {
    pub fn new(total_bytes: u64) -> Result<Self, String> {
        if !(128 * MIB..=MAX_BUDGET_BYTES).contains(&total_bytes) {
            return Err("Container budget must be between 128 MiB and 25 decimal GB.".into());
        }
        let virtual_disk_bytes = ((total_bytes - 64 * MIB) / MIB) * MIB;
        Ok(Self {
            total_bytes,
            virtual_disk_bytes,
            reserved_container_overhead_bytes: total_bytes - virtual_disk_bytes,
        })
    }
    pub fn validate_file_size(&self, bytes: u64) -> Result<(), String> {
        let expected = Self::new(self.total_bytes)?;
        if self.virtual_disk_bytes != expected.virtual_disk_bytes
            || self.reserved_container_overhead_bytes != expected.reserved_container_overhead_bytes
        {
            return Err("Invalid or altered container budget layout.".into());
        }
        // Two independently checksummed journal pages survive an interrupted update.
        if bytes < self.virtual_disk_bytes || bytes > self.total_bytes - journal::JOURNAL_BYTES {
            Err("VHDX allocation exceeds its complete SSD budget or is not fully allocated.".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeIdentity {
    pub volume_guid: String,
    pub disk_number: u32,
    pub extent_offset: u64,
    pub extent_length: u64,
    pub bus_type: u32,
    pub serial: String,
    pub removable: bool,
    pub incurs_seek_penalty: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub flags: u32,
    pub provider_flags: u32,
    pub device_id: u32,
    pub microsoft_provider: bool,
    pub ancestor_level: u32,
    pub dependency_device: String,
    pub host_volume: String,
    pub dependent_volume: String,
    pub relative_path: String,
}

/// A deliberately narrow first topology: direct ATA/SATA HDD, separate direct
/// SATA/NVMe SSD and a standalone Microsoft VHDX hosted by that exact SSD volume.
/// Encryption, system-volume roles, health, ownership and production qualification
/// require separate checks; passing this function does not authorize activation.
pub fn validate_separation(
    source: &VolumeIdentity,
    parent: &VolumeIdentity,
    dependencies: &[Dependency],
) -> Result<(), String> {
    let direct_source = matches!(source.bus_type, 3 | 11); // ATA / SATA
    let direct_ssd = matches!(parent.bus_type, 3 | 11 | 17); // plus NVMe
    if !direct_source
        || !direct_ssd
        || source.removable
        || parent.removable
        || source.incurs_seek_penalty != Some(true)
        || parent.incurs_seek_penalty != Some(false)
        || source.serial.is_empty()
        || parent.serial.is_empty()
        || source.disk_number == parent.disk_number
        || source.serial.eq_ignore_ascii_case(&parent.serial)
        || same_volume(&source.volume_guid, &parent.volume_guid)
        || source.extent_length == 0
        || parent.extent_length == 0
    {
        return Err("Require positively identified, distinct direct HDD and SSD devices.".into());
    }
    let [dependency] = dependencies else {
        return Err("Missing, nested or ambiguous VHDX backing dependencies.".into());
    };
    if !dependency.microsoft_provider
        || dependency.device_id != 3
        || dependency.provider_flags != 0
        || dependency.ancestor_level != 0
        || !same_volume(&dependency.host_volume,&parent.volume_guid)
        || same_volume(&dependency.host_volume,&source.volume_guid)
        // Only the ordinary fully-allocated standalone VHDX flags observed and
        // documented for our container are accepted. Differencing/remote and
        // system/special dependency flags require separate support.
        // REMOVABLE describes the handle-lifetime virtual disk. The physical
        // parent's nonremovable status and direct SSD bus are checked above.
        || dependency.flags & !(2 | 64 | 128) != 0
        || dependency.flags & 2 == 0
    {
        return Err(
            "VHDX does not resolve exclusively to the selected physical SSD volume.".into(),
        );
    }
    Ok(())
}

/// Verify the backing alone before writing to our newly created virtual disk.
pub fn validate_backing(
    parent: &VolumeIdentity,
    dependencies: &[Dependency],
) -> Result<(), String> {
    if parent.removable
        || parent.incurs_seek_penalty != Some(false)
        || !matches!(parent.bus_type, 3 | 11 | 17)
        || parent.serial.is_empty()
        || parent.extent_length == 0
    {
        return Err("Require a positively identified direct SSD backing volume.".into());
    }
    let [dependency] = dependencies else {
        return Err("Missing or nested VHDX backing.".into());
    };
    if !dependency.microsoft_provider
        || dependency.device_id != 3
        || dependency.provider_flags != 0
        || dependency.ancestor_level != 0
        || !same_volume(&dependency.host_volume, &parent.volume_guid)
        || dependency.flags & !(2 | 64 | 128) != 0
        || dependency.flags & 2 == 0
    {
        return Err("Virtual disk does not resolve exclusively to its recorded SSD.".into());
    }
    Ok(())
}

fn same_volume(left: &str, right: &str) -> bool {
    left.strip_suffix('\\')
        .unwrap_or(left)
        .eq_ignore_ascii_case(right.strip_suffix('\\').unwrap_or(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identities() -> (VolumeIdentity, VolumeIdentity, Dependency) {
        let source = VolumeIdentity {
            volume_guid: "\\\\?\\Volume{00000000-0000-0000-0000-000000000001}\\".into(),
            disk_number: 2,
            extent_offset: MIB,
            extent_length: 1024 * MIB,
            bus_type: 11,
            serial: "HDD-001".into(),
            removable: false,
            incurs_seek_penalty: Some(true),
        };
        let mut parent = source.clone();
        parent.volume_guid = "\\\\?\\Volume{00000000-0000-0000-0000-000000000002}\\".into();
        parent.disk_number = 3;
        parent.bus_type = 17;
        parent.serial = "SSD-002".into();
        parent.incurs_seek_penalty = Some(false);
        let dependency = Dependency {
            flags: 2 | 64 | 128,
            provider_flags: 0,
            device_id: 3,
            microsoft_provider: true,
            ancestor_level: 0,
            dependency_device: "\\\\.\\PhysicalDrive9".into(),
            host_volume: parent.volume_guid.clone(),
            dependent_volume: String::new(),
            relative_path: "\\SuperHDD\\cache.vhdx".into(),
        };
        (source, parent, dependency)
    }
    #[test]
    fn budget_accounts_for_container_and_journal_in_decimal_limit() {
        assert!(ContainerBudget::new(MAX_BUDGET_BYTES + 1).is_err());
        assert!(ContainerBudget::new(127 * MIB).is_err());
        let budget = ContainerBudget::new(MAX_BUDGET_BYTES).unwrap();
        assert_eq!(budget.virtual_disk_bytes % MIB, 0);
        assert!(budget.reserved_container_overhead_bytes >= 64 * MIB);
        assert!(budget
            .validate_file_size(budget.virtual_disk_bytes + 32 * MIB)
            .is_ok());
        assert!(budget.validate_file_size(MAX_BUDGET_BYTES).is_err());
        assert!(budget.validate_file_size(4096).is_err());
    }
    #[test]
    fn backing_on_source_or_shared_physical_device_is_rejected() {
        let (source, mut parent, mut dependency) = identities();
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_ok());
        parent.disk_number = source.disk_number;
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_err());
        parent.disk_number = 3;
        dependency.host_volume = source.volume_guid.clone();
        assert!(validate_separation(&source, &parent, &[dependency]).is_err());
    }
    #[test]
    fn nested_missing_and_remote_backing_are_not_treated_as_separate_ssd() {
        let (source, parent, mut dependency) = identities();
        assert!(validate_separation(&source, &parent, &[]).is_err());
        assert!(
            validate_separation(&source, &parent, &[dependency.clone(), dependency.clone()])
                .is_err()
        );
        dependency.flags = 8; // REMOTE
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_err());
        dependency.flags = 2;
        dependency.ancestor_level = 2;
        assert!(validate_separation(&source, &parent, &[dependency]).is_err());
    }
    #[test]
    fn unknown_media_identity_is_not_a_successful_topology_proof() {
        let (mut source, mut parent, dependency) = identities();
        source.incurs_seek_penalty = None;
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_err());
        source.incurs_seek_penalty = Some(true);
        parent.serial.clear();
        assert!(validate_separation(&source, &parent, &[dependency]).is_err());
    }
    #[test]
    fn actual_windows_vhdx_dependency_shape_keeps_exact_parent_validation() {
        let (source, parent, mut dependency) = identities();
        dependency.host_volume = parent.volume_guid.strip_suffix('\\').unwrap().to_string();
        dependency.ancestor_level = 0;
        dependency.flags = 194;
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_ok());
        dependency.host_volume = source.volume_guid.strip_suffix('\\').unwrap().to_string();
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_err());
        dependency.host_volume = parent.volume_guid.clone();
        dependency.flags |= 8;
        assert!(validate_separation(&source, &parent, std::slice::from_ref(&dependency)).is_err());
    }
}
