//! Native owned-container/session integration. The current driver is restricted
//! to disposable file-backed sources; this does not expose physical activation.
use crate::raw_lab::{Attachment, Controller};
use std::path::Path;
use superhdd_storage_control::{
    journal::Phase as ContainerPhase,
    session::{Backend, DriverState, Session, Target},
    validate_separation,
    windows::{
        disk_dependencies, volume_device, volume_filesystem, volume_identity, OwnedVhd, VerifiedLab,
    },
    ContainerBudget,
};
pub struct NativeBackend {
    driver: Controller,
    container: Option<OwnedVhd>,
    source_device: String,
    source_volume: String,
    source_disk: u32,
    source_parent: superhdd_storage_control::VolumeIdentity,
    attached: bool,
    lab: Option<VerifiedLab>,
}
pub type ManagedCache = Session<NativeBackend>;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryOutcome {
    pub kernel_disabled: bool,
    pub stale_backing_detached: bool,
}
/// Reconcile only an immutable, protected checkpoint. File identity, native
/// ancestry, the single-partition source and current kernel identities are all
/// verified before disable. A changed/foreign session is never modified.
pub fn recover_isolated_lab(
    source: &str,
    root: &Path,
    identity: superhdd_storage_control::lab::LabIdentity,
    target: &Target,
) -> Result<RecoveryOutcome, String> {
    use superhdd_storage_control::windows::validate_single_partition;
    let lab = VerifiedLab::verify(identity)?;
    lab.validate_root(root)?;
    let source_identity = volume_identity(source)?;
    if source_identity.bus_type != 15
        || source_identity.removable
        || volume_filesystem(source)? != "NTFS"
        || !source_identity
            .volume_guid
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(target.source_volume.trim_end_matches('\\'))
    {
        return Err("Recorded recovery source differs; preserved.".into());
    }
    validate_single_partition(&source_identity)?;
    let dependencies = disk_dependencies(&format!(
        "\\\\.\\PhysicalDrive{}",
        source_identity.disk_number
    ))?;
    let [dependency] = dependencies.as_slice() else {
        return Err("Missing recovery source ancestry".into());
    };
    let parent = volume_identity(&format!(
        "{}\\",
        dependency.host_volume.trim_end_matches('\\')
    ))?;
    lab.validate_source_backing(&parent, &dependencies)?;
    let mut container = OwnedVhd::reopen_in_lab_service(root, lab.clone())?;
    lab.validate_pair(&parent, &container.parent)?;
    if container.instance() != target.container_instance {
        return Err("Recovered container instance changed; preserved.".into());
    }
    let observed = container.inspect_attached_raw()?;
    let driver = Controller::open()?;
    let status = driver.status()?;
    let mut kernel_disabled = false;
    if status.cache.attached {
        let (disk, bytes) =
            observed.ok_or("Kernel attached but owned backing cannot be observed; preserved")?;
        let (payload, total) = superhdd_storage_control::format::layout(bytes)?;
        if status.source_disk_number != source_identity.disk_number
            || status.backing_disk_number != disk
            || bytes != target.capacity_bytes
            || status.cache.capacity_bytes != payload
            || status.allocated_bytes != total
            || volume_device(&target.source_volume)? != target.source_device
            || volume_device(&target.cache_volume)? != target.cache_device
        {
            return Err(
                "Kernel session differs from protected ownership; no disable was sent.".into(),
            );
        }
        driver.disable()?;
        if driver.status()?.cache.attached {
            return Err("Kernel retained recovery backing; preserved.".into());
        }
        kernel_disabled = true;
    }
    if let Some((disk, _)) = observed {
        container.detach_observed_raw(disk)?;
    }
    Ok(RecoveryOutcome {
        kernel_disabled,
        stale_backing_detached: observed.is_some(),
    })
}

/// Check the loaded driver and the lab source before allocating/opening backing.
pub fn open(
    source: &str,
    root: &Path,
    budget: Option<ContainerBudget>,
) -> Result<ManagedCache, String> {
    open_with_policy(source, root, budget, None, false)
}
pub fn open_isolated_lab(
    source: &str,
    root: &Path,
    budget: Option<ContainerBudget>,
    identity: superhdd_storage_control::lab::LabIdentity,
) -> Result<ManagedCache, String> {
    let lab = VerifiedLab::verify(identity)?;
    lab.validate_root(root)?;
    open_with_policy(source, root, budget, Some(lab), false)
}
pub fn open_isolated_service(
    source: &str,
    root: &Path,
    budget: Option<ContainerBudget>,
    identity: superhdd_storage_control::lab::LabIdentity,
) -> Result<ManagedCache, String> {
    let lab = VerifiedLab::verify(identity)?;
    lab.validate_root(root)?;
    open_with_policy(source, root, budget, Some(lab), true)
}
fn open_with_policy(
    source: &str,
    root: &Path,
    budget: Option<ContainerBudget>,
    lab: Option<VerifiedLab>,
    persistent: bool,
) -> Result<ManagedCache, String> {
    let driver = Controller::open()?;
    if driver.status()?.cache.attached {
        return Err("Another native cache is already attached.".into());
    }
    let source_identity = volume_identity(source)?;
    if source_identity.bus_type != 15
        || source_identity.removable
        || volume_filesystem(source)? != "NTFS"
    {
        return Err("The current driver accepts only disposable file-backed NTFS sources; physical activation remains unavailable.".into());
    }
    let source_dependencies = disk_dependencies(&format!(
        "\\\\.\\PhysicalDrive{}",
        source_identity.disk_number
    ))?;
    let [dependency] = source_dependencies.as_slice() else {
        return Err("Missing or nested source-file ancestry.".into());
    };
    if lab.is_none()
        && (!dependency.microsoft_provider
            || dependency.device_id != 3
            || dependency.ancestor_level != 0
            || dependency.provider_flags != 0
            || dependency.flags & !(2 | 64 | 128) != 0
            || dependency.flags & 2 == 0)
    {
        return Err("Unproven disposable-source ancestry.".into());
    }
    let source_parent = volume_identity(&format!(
        "{}\\",
        dependency.host_volume.trim_end_matches('\\')
    ))?;
    let source_device = volume_device(source)?;
    if let Some(lab) = &lab {
        lab.validate_source_backing(&source_parent, &source_dependencies)?;
    }
    let mut container = if persistent {
        let lab = lab
            .as_ref()
            .ok_or("Persistent cache requires a verified protected service owner")?;
        match budget {
            Some(budget) => OwnedVhd::create_in_lab_service(root, budget, lab.clone())?,
            None => OwnedVhd::reopen_in_lab_service(root, lab.clone())?,
        }
    } else {
        match (&lab, budget) {
            (None, Some(budget)) => OwnedVhd::create(root, budget)?,
            (None, None) => OwnedVhd::reopen(root)?,
            (Some(lab), Some(budget)) => OwnedVhd::create_in_lab(root, budget, lab.clone())?,
            (Some(lab), None) => OwnedVhd::reopen_in_lab(root, lab.clone())?,
        }
    };
    if persistent && container.phase() == ContainerPhase::RawReady {
        // An interrupted prepare can leave a durable attachment without a
        // configured driver. Release only this verified owned orphan.
        if let Some((disk, _)) = container.inspect_attached_raw()? {
            if driver.status()?.cache.attached {
                return Err("Another kernel session appeared; owned orphan preserved.".into());
            }
            container.detach_observed_raw(disk)?;
        }
    }
    Ok(Session::new(NativeBackend {
        driver,
        container: Some(container),
        source_device,
        source_volume: source_identity.volume_guid,
        source_disk: source_identity.disk_number,
        source_parent,
        attached: false,
        lab,
    }))
}
impl Backend for NativeBackend {
    fn status(&mut self) -> Result<DriverState, String> {
        let status = self.driver.status()?;
        Ok(DriverState {
            attached: status.cache.attached,
            paused: status.cache.paused,
            bypass: status.cache.cache_disabled,
            source_disk: status.cache.attached.then_some(status.source_disk_number),
            cache_disk: status.cache.attached.then_some(status.backing_disk_number),
        })
    }
    fn prepare_backing(&mut self) -> Result<Target, String> {
        let container = self
            .container
            .as_mut()
            .ok_or("Backing was preserved after a shutdown error")?;
        let physical = container.attach()?;
        self.attached = true;
        if let Some(lab) = &self.lab {
            lab.validate_pair(&self.source_parent, &container.parent)?;
        } else {
            validate_separation(
                &self.source_parent,
                &container.parent,
                &disk_dependencies(&physical)?,
            )?;
        }
        let raw = match container.phase() {
            ContainerPhase::Created => container.initialize_raw()?,
            ContainerPhase::RawReady => container.raw_volume()?,
            _ => return Err("Incomplete backing initialization; preserving its files.".into()),
        };
        Ok(Target {
            source_device: self.source_device.clone(),
            cache_device: raw.device_path,
            source_disk: self.source_disk,
            cache_disk: raw.disk_number,
            capacity_bytes: raw.capacity_bytes,
            source_volume: self.source_volume.clone(),
            cache_volume: raw.volume_guid,
            container_instance: container.instance(),
        })
    }
    fn configure(&mut self, target: &Target) -> Result<(), String> {
        self.driver.configure(&Attachment {
            source_device: target.source_device.clone(),
            cache_device: target.cache_device.clone(),
            capacity_bytes: target.capacity_bytes,
            acknowledged_disposable_vhds: true,
        })
    }
    fn pause(&mut self, paused: bool) -> Result<(), String> {
        self.driver.pause(paused)
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
                .ok_or("Missing owned backing")?
                .detach()?;
            self.attached = false;
        }
        Ok(())
    }
    fn preserve_backing(&mut self) {
        // Do not invoke OwnedVhd's detach destructor while the kernel still holds
        // the session. This bounded exceptional lease lasts until process exit;
        // crash/PnP qualification is still required before any production release.
        if let Some(container) = self.container.take() {
            std::mem::forget(container);
        }
    }
}
