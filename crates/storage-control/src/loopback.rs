//! Protected service-to-worker contract. No disk is offlined or formatted here.
//! A worker may open a source only after the exact prepared journal is durable.
use crate::{
    data_source::{self, DiskIdentity},
    journal::FileIdentity,
    recovery::{config_digest, Phase, RunJournal},
    session::Target,
    windows::{self, protected::ProtectedDir},
    VolumeIdentity, MAX_BUDGET_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::*;
pub const ROOT: &str = r"C:\ProgramData\SuperHDDLoopback";
type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabSource {
    pub file: FileIdentity,
    pub host_volume: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema: u32,
    pub run: String,
    pub source: DiskIdentity,
    pub source_volume: VolumeIdentity,
    pub parent: VolumeIdentity,
    pub capacity_bytes: u64,
    pub payload: FileIdentity,
    pub source_fully_decrypted: bool,
    pub lab: Option<LabSource>,
}
fn same(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}
impl Config {
    pub fn instance(&self) -> Result<[u8; 16]> {
        crate::iscsi::target_name(&self.run)?;
        let mut id = [0; 16];
        for (i, byte) in id.iter_mut().enumerate() {
            *byte =
                u8::from_str_radix(&self.run[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
        }
        if id == [0; 16] {
            return Err("Missing native runtime instance".into());
        }
        Ok(id)
    }
    pub fn validate(&self) -> Result<()> {
        self.instance()?;
        let minimum = if self.lab.is_some() {
            32 * 1024 * 1024
        } else {
            1024 * 1024 * 1024
        };
        if self.schema != 1
            || !self.source_fully_decrypted
            || self.capacity_bytes < minimum
            || self.capacity_bytes > MAX_BUDGET_BYTES
            || !self.capacity_bytes.is_multiple_of(65536)
            || self.source.logical_sector_bytes != 512
            || self.source.bytes == 0
            || self.source.bytes > i64::MAX as u64
            || !self.source.bytes.is_multiple_of(4096)
            || self.source.gpt_id == [0; 16]
            || self.source.partition_id == [0; 16]
            || self.source.partition_bytes == 0
            || self
                .source
                .partition_offset
                .checked_add(self.source.partition_bytes)
                .is_none_or(|end| end > self.source.bytes)
            || self.source_volume.disk_number != self.source.number
            || self.source_volume.extent_offset != self.source.partition_offset
            || self.source_volume.extent_length != self.source.partition_bytes
            || self.source_volume.serial != self.source.serial
            || self.source_volume.bus_type != self.source.bus_type
            || self.source_volume.removable
            || self.parent.removable
            || self.parent.serial.is_empty()
            || self.parent.incurs_seek_penalty != Some(false)
            || !matches!(self.parent.bus_type, 3 | 11 | 17)
            || self.parent.disk_number == self.source.number
            || same(&self.parent.volume_guid, &self.source_volume.volume_guid)
            || self.payload.index == 0
            || self.payload.created == 0
        {
            return Err("Invalid native runtime source, cache identity, role or budget".into());
        }
        for volume in [&self.source_volume.volume_guid, &self.parent.volume_guid] {
            if !same(&windows::volume_guid(volume)?, volume) || !volume.starts_with(r"\\?\Volume{")
            {
                return Err("Canonical recorded volume GUID required".into());
            }
        }
        if let Some(lab) = &self.lab {
            if self.source.bus_type != 15
                || self.source.bytes != 128 * 1024 * 1024
                || self.capacity_bytes != 32 * 1024 * 1024
                || lab.file.index == 0
                || lab.file.created == 0
                || !same(&windows::volume_guid(&lab.host_volume)?, &lab.host_volume)
                || same(&lab.host_volume, &self.parent.volume_guid)
            {
                return Err(
                    "Disposable role accepts only an identified 128 MiB file-backed VHDX".into(),
                );
            }
        } else if !matches!(self.source.bus_type, 3 | 11)
            || self.source.serial.is_empty()
            || self.source_volume.incurs_seek_penalty != Some(true)
            || self.source.serial.eq_ignore_ascii_case(&self.parent.serial)
        {
            return Err(
                "Physical role requires a positively identified separate direct HDD".into(),
            );
        }
        Ok(())
    }
    pub fn cache_root(&self) -> Result<PathBuf> {
        self.validate()?;
        Ok(PathBuf::from(&self.parent.volume_guid)
            .join("SuperHDDCache")
            .join(&self.run))
    }
    pub fn target(&self) -> Result<Target> {
        Ok(Target {
            source_device: format!("\\\\.\\PhysicalDrive{}", self.source.number),
            cache_device: self
                .cache_root()?
                .join("payload.bin")
                .to_str()
                .ok_or("Invalid payload path")?
                .into(),
            source_disk: self.source.number,
            cache_disk: self.parent.disk_number,
            capacity_bytes: self.capacity_bytes,
            source_volume: self.source_volume.volume_guid.clone(),
            cache_volume: self.parent.volume_guid.clone(),
            container_instance: self.instance()?,
        })
    }
}
pub struct Pins {
    pub reservation: crate::port_reservation::Reservation,
    pub config: Config,
    pub root: ProtectedDir,
    pub cache: ProtectedDir,
    _manifest: File,
    _port_owner: File,
    _lab: Option<File>,
}
pub struct Opened {
    pub source: File,
    pub payload: File,
    pub pins: Pins,
}
impl Opened {
    pub fn open(run: &str) -> Result<Self> {
        crate::iscsi::target_name(run)?;
        let root = ProtectedDir::open(Path::new(ROOT))?;
        let mut manifest = root.read_file("manifest.json")?;
        let mut bytes = Vec::new();
        (&mut manifest)
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 32768 {
            return Err("Oversized native runtime manifest".into());
        }
        let config: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        config.validate()?;
        if config.run != run {
            return Err("Native worker instance differs from protected owner".into());
        }
        let mut port_owner = root.read_file("port-owner.json")?;
        let mut port_bytes = Vec::new();
        (&mut port_owner)
            .take(1025)
            .read_to_end(&mut port_bytes)
            .map_err(|e| e.to_string())?;
        if port_bytes.len() > 1024 {
            return Err("Oversized endpoint owner record".into());
        }
        let reservation: crate::port_reservation::Reservation =
            serde_json::from_slice(&port_bytes).map_err(|e| e.to_string())?;
        reservation.validate()?;
        let journal = RunJournal::open(root.read_shared_file("lease.bin")?, config_digest(&bytes))?;
        let target = config.target()?;
        if !matches!(journal.record().phase, Phase::Prepared | Phase::Active)
            || journal.record().target.as_ref() != Some(&target)
        {
            return Err(
                "Native worker requires the exact durable prepared target before source access"
                    .into(),
            );
        }
        let parent = windows::volume_identity(&config.parent.volume_guid)?;
        if serde_json::to_value(&parent).map_err(|e| e.to_string())?
            != serde_json::to_value(&config.parent).map_err(|e| e.to_string())?
            || windows::volume_filesystem(&parent.volume_guid)? != "NTFS"
        {
            return Err("Native cache parent changed".into());
        }
        let lab = if let Some(lab) = &config.lab {
            let file = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(PathBuf::from(&lab.host_volume).join(format!("SuperHDD-handoff-{run}.vhdx")))
                .map_err(|e| e.to_string())?;
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            let deps = windows::disk_dependencies(&target.source_device)?;
            let [dependency] = deps.as_slice() else {
                return Err("Ambiguous disposable source dependency".into());
            };
            if !metadata.is_file()
                || metadata.file_attributes()
                    & (FILE_ATTRIBUTE_REPARSE_POINT
                        | FILE_ATTRIBUTE_COMPRESSED
                        | FILE_ATTRIBUTE_ENCRYPTED)
                    != 0
                || windows::stable_file_identity(&file)? != lab.file
                || !dependency.microsoft_provider
                || dependency.device_id != 3
                || dependency.ancestor_level != 0
                || dependency.provider_flags != 0
                || !same(&dependency.host_volume, &lab.host_volume)
                || !dependency
                    .relative_path
                    .eq_ignore_ascii_case(&format!("\\SuperHDD-handoff-{run}.vhdx"))
            {
                return Err(
                    "Disposable source cannot resolve to a physical HDD or another file".into(),
                );
            }
            Some(file)
        } else {
            None
        };
        let cache = ProtectedDir::open(&config.cache_root()?)?;
        let payload = cache.payload("payload.bin", false)?;
        if windows::stable_file_identity(&payload)? != config.payload
            || payload.metadata().map_err(|e| e.to_string())?.len() != config.capacity_bytes
        {
            return Err("Protected SSD payload identity or allocation length changed".into());
        }
        let source = data_source::open_offline_device(&config.source)?;
        Ok(Self {
            source,
            payload,
            pins: Pins {
                reservation,
                config,
                root,
                cache,
                _manifest: manifest,
                _port_owner: port_owner,
                _lab: lab,
            },
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        let source = VolumeIdentity {
            volume_guid: r"\\?\Volume{00000000-0000-0000-0000-000000000001}\".into(),
            disk_number: 2,
            extent_offset: 1024 * 1024,
            extent_length: 1_000_000_000_000,
            bus_type: 11,
            serial: "hdd".into(),
            removable: false,
            incurs_seek_penalty: Some(true),
        };
        let mut parent = source.clone();
        parent.disk_number = 3;
        parent.serial = "ssd".into();
        parent.volume_guid = r"\\?\Volume{00000000-0000-0000-0000-000000000002}\".into();
        parent.bus_type = 17;
        parent.incurs_seek_penalty = Some(false);
        Config {
            schema: 1,
            run: "0123456789abcdef0123456789abcdef".into(),
            source: DiskIdentity {
                number: 2,
                bytes: 2_000_000_000_000,
                logical_sector_bytes: 512,
                bus_type: 11,
                serial: "hdd".into(),
                gpt_id: [1; 16],
                partition_id: [2; 16],
                partition_offset: source.extent_offset,
                partition_bytes: source.extent_length,
            },
            source_volume: source,
            parent,
            capacity_bytes: 24_999_952_384,
            payload: FileIdentity {
                volume_serial: 3,
                index: 17,
                created: 100,
            },
            source_fully_decrypted: true,
            lab: None,
        }
    }
    #[test]
    fn runtime_config_rejects_overlap_altered_roles_encryption_and_budget() {
        let original = config();
        assert!(original.validate().is_ok());
        assert_eq!(original.target().unwrap().capacity_bytes, 24_999_952_384);
        let mut c = original.clone();
        c.parent.disk_number = 2;
        assert!(c.validate().is_err());
        let mut c = original.clone();
        c.source_fully_decrypted = false;
        assert!(c.validate().is_err());
        let mut c = original.clone();
        c.capacity_bytes = 25_000_017_920;
        assert!(c.validate().is_err());
        let mut c = original.clone();
        c.run = "../escape".into();
        assert!(c.validate().is_err());
        let mut c = original.clone();
        c.lab = Some(LabSource {
            file: c.payload.clone(),
            host_volume: c.source_volume.volume_guid.clone(),
        });
        assert!(c.validate().is_err());
        let mut c = original;
        c.source.partition_bytes = u64::MAX;
        assert!(c.validate().is_err());
    }
}
