//! Read-only enrollment checks for the native Windows initiator backend.
//! Passing preflight does not offline a disk, allocate SSD space or enable cache.
use crate::{CachePlan, CacheRequest, Snapshot};
use serde::{Deserialize, Serialize};
use superhdd_storage_control::{
    data_source::{self, DiskIdentity},
    windows, VolumeIdentity,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub schema: u32,
    pub request: CacheRequest,
    pub source: VolumeIdentity,
    pub parent: VolumeIdentity,
    pub disk: DiskIdentity,
}
fn hardware_plan(snapshot: &Snapshot, request: &CacheRequest) -> Result<CachePlan, String> {
    crate::cache_runtime::Command {
        operation: crate::cache_runtime::Operation::Start,
        request: Some(request.clone()),
    }
    .validate()?;
    let plan = crate::plan_cache(snapshot, request.clone())?;
    let source = snapshot
        .disks
        .iter()
        .find(|d| d.id == request.source_disk_id)
        .ok_or("Source absent")?;
    let parent = snapshot
        .disks
        .iter()
        .find(|d| d.id == request.ssd_disk_id)
        .ok_or("Cache parent absent")?;
    if source.number == parent.number || source.system || parent.system {
        return Err(
            "Whole-disk handoff requires two separate data disks without boot/system roles".into(),
        );
    }
    let volume = source
        .volumes
        .iter()
        .find(|v| v.letter == request.source_letter)
        .ok_or("Source volume absent")?;
    if volume.encryption != "FullyDecrypted" {
        return Err("Source encryption must be positively verified FullyDecrypted".into());
    }
    Ok(plan)
}
impl Context {
    pub fn capture(request: CacheRequest) -> Result<Self, String> {
        let snapshot = crate::discover()?;
        hardware_plan(&snapshot, &request)?;
        let source = windows::volume_identity(&request.source_letter)?;
        let parent = windows::volume_identity(&request.ssd_letter)?;
        let selected_source = snapshot
            .disks
            .iter()
            .find(|d| d.id == request.source_disk_id)
            .ok_or("Source absent")?;
        let selected_parent = snapshot
            .disks
            .iter()
            .find(|d| d.id == request.ssd_disk_id)
            .ok_or("Cache parent absent")?;
        if source.disk_number != selected_source.number
            || parent.disk_number != selected_parent.number
            || !source
                .serial
                .eq_ignore_ascii_case(selected_source.serial.trim())
            || !parent
                .serial
                .eq_ignore_ascii_case(selected_parent.serial.trim())
            || !matches!(source.bus_type, 3 | 11)
            || !matches!(parent.bus_type, 3 | 11 | 17)
            || source.removable
            || parent.removable
            || source.serial.is_empty()
            || parent.serial.is_empty()
            || source.serial.eq_ignore_ascii_case(&parent.serial)
            || source.disk_number == parent.disk_number
            || source.volume_guid.eq_ignore_ascii_case(&parent.volume_guid)
            || source.incurs_seek_penalty != Some(true)
            || parent.incurs_seek_penalty != Some(false)
            || windows::volume_filesystem(&source.volume_guid)? != "NTFS"
            || windows::volume_filesystem(&parent.volume_guid)? != "NTFS"
        {
            return Err(
                "Native HDD/SSD identities differ from the selected separate direct data disks"
                    .into(),
            );
        }
        let disk = data_source::inspect_volume(&source)?;
        if disk.bytes != selected_source.size || disk.logical_sector_bytes != 512 {
            return Err(
                "Initial native runtime requires the exact GPT source with 512-byte sectors".into(),
            );
        }
        Ok(Self {
            schema: 1,
            request,
            source,
            parent,
            disk,
        })
    }
    pub fn revalidate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("Unknown native runtime context".into());
        }
        let now = Self::capture(self.request.clone())?;
        let equal = |a: &VolumeIdentity, b: &VolumeIdentity| {
            a.volume_guid.eq_ignore_ascii_case(&b.volume_guid)
                && a.disk_number == b.disk_number
                && a.extent_offset == b.extent_offset
                && a.extent_length == b.extent_length
                && a.bus_type == b.bus_type
                && a.serial == b.serial
                && a.removable == b.removable
                && a.incurs_seek_penalty == b.incurs_seek_penalty
        };
        if now.disk != self.disk
            || !equal(&now.source, &self.source)
            || !equal(&now.parent, &self.parent)
        {
            return Err(
                "Native runtime source/cache identity changed; existing storage preserved".into(),
            );
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Snapshot, CacheRequest) {
        let volume = crate::Volume {
            letter: "D:".into(),
            label: "data".into(),
            file_system: "NTFS".into(),
            size: 1_000_000_000_000,
            free: 500_000_000_000,
            partition_number: 2,
            system: false,
            health: "Healthy".into(),
            encryption: "FullyDecrypted".into(),
        };
        let hdd = crate::Disk {
            number: 2,
            id: "hdd-id".into(),
            model: "HDD".into(),
            serial: "hdd-serial".into(),
            kind: "HDD".into(),
            media_evidence: "test".into(),
            bus: "SATA".into(),
            size: 1_000_000_000_000,
            online: true,
            read_only: false,
            system: false,
            volumes: vec![volume.clone()],
        };
        let mut ssd = hdd.clone();
        ssd.number = 3;
        ssd.id = "ssd-id".into();
        ssd.serial = "ssd-serial".into();
        ssd.kind = "SSD".into();
        ssd.volumes[0].letter = "M:".into();
        (
            Snapshot {
                disks: vec![hdd, ssd],
                drivers: vec![],
                service_state: "NotInstalled".into(),
                collected_at: "test".into(),
            },
            CacheRequest {
                source_disk_id: "hdd-id".into(),
                source_letter: "D:".into(),
                ssd_disk_id: "ssd-id".into(),
                ssd_letter: "M:".into(),
                capacity_bytes: 24_999_952_384,
                scope: None,
            },
        )
    }
    #[test]
    fn native_hardware_preflight_is_independent_of_unsigned_driver_gate() {
        let (s, r) = fixture();
        let plan = hardware_plan(&s, &r).unwrap();
        assert!(plan.reasons.contains(&"driverNotLoaded".into()));
        assert_eq!(plan.request.capacity_bytes, 24_999_952_384);
        // Preflight deliberately leaves the desktop activation gate unchanged.
        assert_eq!(
            plan.activation_available,
            crate::cache_runtime::qualified_driver_hash().is_some()
        );
    }
    #[test]
    fn native_whole_disk_path_refuses_system_roles_aliases_and_unknown_encryption() {
        let (s, r) = fixture();
        let mut altered = s.clone();
        altered.disks[0].system = true;
        assert!(hardware_plan(&altered, &r).is_err());
        let mut altered = s.clone();
        altered.disks[1].number = 2;
        assert!(hardware_plan(&altered, &r).is_err());
        let mut altered = s;
        altered.disks[0].volumes[0].encryption = "Unknown".into();
        assert!(hardware_plan(&altered, &r).is_err());
    }
}
