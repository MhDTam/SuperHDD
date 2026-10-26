//! Direct HDD sessions with a private VHDX on a different proven SSD.
//! This module never partitions or formats a physical disk.
use crate::transparent_driver::{Attachment, Controller};
use serde::{Deserialize, Serialize};
use std::path::Path;
use superhdd_storage_control::{
    journal::Phase,
    lab::LabIdentity,
    session::{Backend, DriverState, Session, Target},
    windows::{volume_device, volume_filesystem, volume_identity, OwnedVhd, VerifiedLab},
    ContainerBudget, VolumeIdentity,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub source: VolumeIdentity,
    pub isolated_lab: Option<LabIdentity>,
}
fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}
impl Context {
    pub fn capture(source: &str, root: &Path, lab: Option<LabIdentity>) -> Result<Self, String> {
        let c = Self {
            source: volume_identity(source)?,
            isolated_lab: lab,
        };
        c.validate(root)?;
        Ok(c)
    }
    fn validate(&self, root: &Path) -> Result<(VolumeIdentity, Option<VerifiedLab>), String> {
        let now = volume_identity(&self.source.volume_guid)?;
        if !same(&now.volume_guid, &self.source.volume_guid)
            || !same(&now.serial, &self.source.serial)
            || now.extent_offset != self.source.extent_offset
            || now.extent_length != self.source.extent_length
            || now.bus_type != self.source.bus_type
            || now.removable
            || volume_filesystem(&now.volume_guid)? != "NTFS"
        {
            return Err("Source identity or extent changed; storage preserved".into());
        }
        let lab = if let Some(id) = &self.isolated_lab {
            let lab = VerifiedLab::verify(id.clone())?;
            lab.validate_root(root)?;
            lab.validate_source_parent(&now)?;
            Some(lab)
        } else {
            if !matches!(now.bus_type, 3 | 11)
                || now.serial.is_empty()
                || now.incurs_seek_penalty != Some(true)
            {
                return Err("Require a positively identified direct ATA/SATA HDD".into());
            }
            let snapshot = crate::discover()?;
            let disk = snapshot
                .disks
                .iter()
                .find(|d| d.number == now.disk_number)
                .ok_or("Source disk absent")?;
            let eligible = disk
                .volumes
                .iter()
                .filter(|v| !v.system && v.health == "Healthy" && v.encryption == "FullyDecrypted")
                .any(|v| {
                    volume_identity(&v.letter)
                        .is_ok_and(|id| same(&id.volume_guid, &now.volume_guid))
                });
            if !disk.online || disk.read_only || !eligible {
                return Err(
                    "Require a healthy decrypted HDD data volume without a system role".into(),
                );
            }
            None
        };
        Ok((now, lab))
    }
}
pub struct DirectBackend {
    driver: Controller,
    container: Option<OwnedVhd>,
    context: Context,
    source: VolumeIdentity,
    source_device: String,
    attached: bool,
}
pub type DirectCache = Session<DirectBackend>;
fn owned(
    root: &Path,
    lab: Option<VerifiedLab>,
    budget: Option<ContainerBudget>,
) -> Result<OwnedVhd, String> {
    match (lab, budget) {
        (Some(l), Some(b)) => OwnedVhd::create_in_lab_service(root, b, l),
        (Some(l), None) => OwnedVhd::reopen_in_lab_service(root, l),
        (None, Some(b)) => OwnedVhd::create_service(root, b),
        (None, None) => OwnedVhd::reopen_service(root),
    }
}
pub fn open(
    context: Context,
    root: &Path,
    budget: Option<ContainerBudget>,
) -> Result<DirectCache, String> {
    let (source, lab) = context.validate(root)?;
    let driver = Controller::open(lab.is_some())?;
    if driver.status()?.cache.attached {
        return Err("Another native session is active".into());
    }
    let mut container = owned(root, lab.clone(), budget)?;
    if let Some(l) = lab {
        l.validate_pair(&source, &container.parent)?;
    }
    if source.disk_number == container.parent.disk_number
        || same(&source.volume_guid, &container.parent.volume_guid)
        || (context.isolated_lab.is_none()
            && !source.serial.is_empty()
            && same(&source.serial, &container.parent.serial))
    {
        return Err("Source and cache backing overlap".into());
    }
    if budget.is_none() && container.phase() == Phase::RawReady {
        if let Some((disk, _)) = container.inspect_attached_raw()? {
            if driver.status()?.cache.attached {
                return Err("Kernel acquired a session during orphan inspection".into());
            }
            container.detach_observed_raw(disk)?;
        }
    }
    let source_device = volume_device(&source.volume_guid)?;
    Ok(Session::new(DirectBackend {
        driver,
        container: Some(container),
        context,
        source,
        source_device,
        attached: false,
    }))
}
pub fn recover(
    context: &Context,
    root: &Path,
    target: &Target,
) -> Result<crate::managed_cache::RecoveryOutcome, String> {
    let (source, lab) = context.validate(root)?;
    if !same(&source.volume_guid, &target.source_volume) {
        return Err("Checkpoint source differs".into());
    }
    let mut container = owned(root, lab, None)?;
    if container.instance() != target.container_instance {
        return Err("Checkpoint container differs".into());
    }
    let observed = container.inspect_attached_raw()?;
    let driver = Controller::open(context.isolated_lab.is_some())?;
    let status = driver.status()?;
    if status.cache.attached {
        let (disk, bytes) = observed.ok_or("Cannot prove kernel backing ownership")?;
        let (payload, total) = superhdd_storage_control::format::layout(bytes)?;
        if status.source_disk_number != source.disk_number
            || status.backing_disk_number != disk
            || bytes != target.capacity_bytes
            || status.cache.capacity_bytes != payload
            || status.allocated_bytes != total
            || volume_device(&target.source_volume)? != target.source_device
            || volume_device(&target.cache_volume)? != target.cache_device
        {
            return Err("Kernel session differs from protected checkpoint; no disable sent".into());
        }
        driver.disable()?;
        if driver.status()?.cache.attached {
            return Err("Kernel retained backing".into());
        }
    }
    if let Some((disk, _)) = observed {
        container.detach_observed_raw(disk)?;
    }
    Ok(crate::managed_cache::RecoveryOutcome {
        kernel_disabled: status.cache.attached,
        stale_backing_detached: observed.is_some(),
    })
}
impl Backend for DirectBackend {
    fn status(&mut self) -> Result<DriverState, String> {
        let s = self.driver.status()?;
        Ok(DriverState {
            attached: s.cache.attached,
            paused: s.cache.paused,
            bypass: s.cache.cache_disabled,
            source_disk: s.cache.attached.then_some(s.source_disk_number),
            cache_disk: s.cache.attached.then_some(s.backing_disk_number),
        })
    }
    fn prepare_backing(&mut self) -> Result<Target, String> {
        let c = self.container.as_mut().ok_or("Missing owned container")?;
        c.attach()?;
        self.attached = true;
        let raw = match c.phase() {
            Phase::Created => c.initialize_raw()?,
            Phase::RawReady => c.raw_volume()?,
            _ => return Err("Incomplete container preserved".into()),
        };
        if raw.disk_number == self.source.disk_number
            || c.parent.disk_number == self.source.disk_number
        {
            return Err("Recursive backing refused".into());
        }
        Ok(Target {
            source_device: self.source_device.clone(),
            cache_device: raw.device_path,
            source_disk: self.source.disk_number,
            cache_disk: raw.disk_number,
            capacity_bytes: raw.capacity_bytes,
            source_volume: self.source.volume_guid.clone(),
            cache_volume: raw.volume_guid,
            container_instance: c.instance(),
        })
    }
    fn configure(&mut self, t: &Target) -> Result<(), String> {
        let c = self.container.as_ref().ok_or("Missing owned container")?;
        let (now, _) = self
            .context
            .validate(c.path().parent().ok_or("Missing cache root")?)?;
        if now.disk_number != self.source.disk_number
            || volume_device(&now.volume_guid)? != t.source_device
        {
            return Err("Source changed before configure".into());
        }
        self.driver.configure(&Attachment {
            source_device: t.source_device.clone(),
            cache_device: t.cache_device.clone(),
            cache_parent_device: volume_device(&c.parent.volume_guid)?,
            capacity_bytes: t.capacity_bytes,
            source_disk: now.disk_number,
            cache_parent_disk: c.parent.disk_number,
            source_offset: now.extent_offset,
            source_bytes: now.extent_length,
            vm_test: self.context.isolated_lab.is_some(),
        })
    }
    fn pause(&mut self, p: bool) -> Result<(), String> {
        self.driver.pause(p)
    }
    fn clear(&mut self) -> Result<(), String> {
        self.driver.clear()
    }
    fn disable(&mut self) -> Result<(), String> {
        self.driver.disable()
    }
    fn release_backing(&mut self) -> Result<(), String> {
        if self.attached {
            self.container
                .as_mut()
                .ok_or("Missing owned container")?
                .detach()?;
            self.attached = false;
        }
        Ok(())
    }
    fn preserve_backing(&mut self) {
        if let Some(c) = self.container.take() {
            std::mem::forget(c)
        }
    }
}
