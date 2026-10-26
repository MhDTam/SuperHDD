//! Bounded, two-slot ownership journal. An interrupted write cannot authorize a
//! different file or cause recovery to format a partly initialized container.
use crate::{ContainerBudget, VolumeIdentity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

const PAGE: usize = 4096;
const HEADER: usize = 64;
const MAGIC: &[u8; 8] = b"SHDDOWN2";
pub const JOURNAL_BYTES: u64 = (2 * PAGE) as u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Creating,
    Created,
    Initializing,
    RawReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileIdentity {
    pub volume_serial: u32,
    pub index: u64,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerRecord {
    pub schema: u32,
    pub file: String,
    pub instance: [u8; 16],
    pub budget: ContainerBudget,
    pub parent: VolumeIdentity,
    pub identity: Option<FileIdentity>,
    pub vhd_id: Option<[u8; 16]>,
    pub phase: Phase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lab: Option<crate::lab::LabIdentity>,
    #[serde(default)]
    pub persistent_lease: bool,
}

impl OwnerRecord {
    pub fn validate(&self) -> Result<(), String> {
        let suffix = self
            .file
            .strip_prefix("cache-")
            .and_then(|s| s.strip_suffix(".vhdx"));
        if self.schema != 2
            || self.instance == [0; 16]
            || !suffix.is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            || self.parent.volume_guid.len() > 128
            || self.parent.serial.len() > 256
            || (self.parent.serial.is_empty() && self.lab.is_none())
            || self.parent.extent_length == 0
            || (self.phase == Phase::Creating) != self.identity.is_none()
            || (self.phase == Phase::Creating) != self.vhd_id.is_none()
            || self.vhd_id == Some([0; 16])
            || self.identity.as_ref().is_some_and(|id| id.index == 0)
        {
            return Err("Invalid container ownership record; preserving its files.".into());
        }
        if let Some(lab) = &self.lab {
            lab.validate()?;
            if !crate::same_volume(&lab.cache_parent_volume, &self.parent.volume_guid) {
                return Err("Lab ownership parent mismatch.".into());
            }
        }
        let budget = ContainerBudget::new(self.budget.total_bytes)?;
        if self.budget.virtual_disk_bytes != budget.virtual_disk_bytes
            || self.budget.reserved_container_overhead_bytes
                != budget.reserved_container_overhead_bytes
        {
            return Err("Altered container budget.".into());
        }
        Ok(())
    }
}

pub struct Journal {
    file: File,
    sequence: u64,
    slot: usize,
    record: OwnerRecord,
}
impl Journal {
    pub fn create(file: File, record: OwnerRecord) -> Result<Self, String> {
        record.validate()?;
        if file.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err("Refusing to initialize a nonempty ownership journal.".into());
        }
        file.set_len(JOURNAL_BYTES).map_err(|e| e.to_string())?;
        let mut journal = Self {
            file,
            sequence: 0,
            slot: 1,
            record: record.clone(),
        };
        journal.write(record)?;
        Ok(journal)
    }
    pub fn open(mut file: File) -> Result<Self, String> {
        if file.metadata().map_err(|e| e.to_string())?.len() != JOURNAL_BYTES {
            return Err("Unknown or incomplete ownership journal; preserving its files.".into());
        }
        let mut pages = [0u8; 2 * PAGE];
        file.read_exact(&mut pages).map_err(|e| e.to_string())?;
        let first = decode(&pages[..PAGE]);
        let second = decode(&pages[PAGE..]);
        let (slot, sequence, record) = match (first, second) {
            (Some((a, record)), Some((b, _))) if a > b => (0, a, record),
            (Some((a, _)), Some((b, record))) if b > a => (1, b, record),
            (Some((n, record)), None) => (0, n, record),
            (None, Some((n, record))) => (1, n, record),
            _ => {
                return Err("Corrupt or ambiguous ownership journal; preserving its files.".into())
            }
        };
        Ok(Self {
            file,
            sequence,
            slot,
            record,
        })
    }
    pub fn record(&self) -> &OwnerRecord {
        &self.record
    }
    pub(crate) fn file(&self) -> &File {
        &self.file
    }
    pub fn created(&mut self, identity: FileIdentity, vhd_id: [u8; 16]) -> Result<(), String> {
        if self.record.phase != Phase::Creating {
            return Err("Container ownership is already recorded.".into());
        }
        let mut next = self.record.clone();
        next.identity = Some(identity);
        next.vhd_id = Some(vhd_id);
        next.phase = Phase::Created;
        self.write(next)
    }
    pub fn advance(&mut self, identity: Option<FileIdentity>, phase: Phase) -> Result<(), String> {
        if !matches!(
            (self.record.phase, phase),
            (Phase::Created, Phase::Initializing) | (Phase::Initializing, Phase::RawReady)
        ) || (self.record.identity.is_some() && identity != self.record.identity)
        {
            return Err("Invalid container lifecycle transition.".into());
        }
        let mut next = self.record.clone();
        next.identity = identity;
        next.phase = phase;
        self.write(next)
    }
    fn write(&mut self, record: OwnerRecord) -> Result<(), String> {
        record.validate()?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Ownership sequence exhausted")?;
        let payload = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if payload.len() > PAGE - HEADER {
            return Err("Oversized ownership record.".into());
        }
        let mut bytes = [0u8; PAGE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&sequence.to_le_bytes());
        bytes[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes[HEADER..HEADER + payload.len()].copy_from_slice(&payload);
        let digest = checksum(&bytes);
        bytes[32..64].copy_from_slice(&digest);
        let slot = 1 - self.slot;
        self.file
            .seek(SeekFrom::Start((slot * PAGE) as u64))
            .map_err(|e| e.to_string())?;
        self.file.write_all(&bytes).map_err(|e| e.to_string())?;
        self.file.sync_all().map_err(|e| e.to_string())?;
        self.sequence = sequence;
        self.slot = slot;
        self.record = record;
        Ok(())
    }
}
fn checksum(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(&bytes[..32]);
    hash.update(&bytes[64..]);
    hash.finalize().into()
}
fn decode(bytes: &[u8]) -> Option<(u64, OwnerRecord)> {
    if bytes.len() != PAGE
        || &bytes[..8] != MAGIC
        || bytes[20..32].iter().any(|b| *b != 0)
        || bytes[32..64] != checksum(bytes)
    {
        return None;
    }
    let sequence = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let length = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    if sequence == 0
        || length == 0
        || length > PAGE - HEADER
        || bytes[HEADER + length..].iter().any(|b| *b != 0)
    {
        return None;
    }
    let record: OwnerRecord = serde_json::from_slice(&bytes[HEADER..HEADER + length]).ok()?;
    record.validate().ok()?;
    Some((sequence, record))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> OwnerRecord {
        OwnerRecord {
            schema: 2,
            file: "cache-123456789abcdef0123456789abcdef0.vhdx".into(),
            instance: [1; 16],
            budget: ContainerBudget::new(256 * 1024 * 1024).unwrap(),
            parent: VolumeIdentity {
                volume_guid: "\\\\?\\Volume{00000000-0000-0000-0000-000000000001}\\".into(),
                disk_number: 3,
                extent_offset: 1048576,
                extent_length: 1024 * 1024 * 1024,
                bus_type: 17,
                serial: "SSD-1".into(),
                removable: false,
                incurs_seek_penalty: Some(false),
            },
            identity: None,
            vhd_id: None,
            lab: None,
            persistent_lease: false,
            phase: Phase::Creating,
        }
    }
    fn identity() -> Option<FileIdentity> {
        Some(FileIdentity {
            volume_serial: 42,
            index: 99,
            created: 1234,
        })
    }
    #[test]
    fn durable_identity_and_phase_survive_reopen() {
        let file = tempfile::tempfile().unwrap();
        let mut journal = Journal::create(file, record()).unwrap();
        journal.created(identity().unwrap(), [3; 16]).unwrap();
        journal.advance(identity(), Phase::Initializing).unwrap();
        journal.advance(identity(), Phase::RawReady).unwrap();
        journal.file.seek(SeekFrom::Start(0)).unwrap();
        let opened = Journal::open(journal.file).unwrap();
        assert_eq!(opened.record.phase, Phase::RawReady);
        assert_eq!(opened.record.identity, identity());
    }
    #[test]
    fn torn_latest_write_preserves_previous_record_without_reformatting() {
        let file = tempfile::tempfile().unwrap();
        let mut journal = Journal::create(file, record()).unwrap();
        journal.created(identity().unwrap(), [3; 16]).unwrap();
        journal.advance(identity(), Phase::Initializing).unwrap();
        let slot = 1 - journal.slot;
        journal
            .file
            .seek(SeekFrom::Start((slot * PAGE) as u64))
            .unwrap();
        journal.file.write_all(&[0x77; 137]).unwrap();
        journal.file.seek(SeekFrom::Start(0)).unwrap();
        let opened = Journal::open(journal.file).unwrap();
        assert_eq!(opened.record.phase, Phase::Initializing);
    }
    #[test]
    fn corrupt_records_and_unknown_versions_do_not_authorize_recovery() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&[0; 2 * PAGE]).unwrap();
        file.rewind().unwrap();
        assert!(Journal::open(file).is_err());
        let mut owner = record();
        owner.schema = 1;
        assert!(Journal::create(tempfile::tempfile().unwrap(), owner).is_err());
    }
    #[test]
    fn ownership_cannot_change_and_initialization_cannot_restart() {
        let mut journal = Journal::create(tempfile::tempfile().unwrap(), record()).unwrap();
        assert!(journal.advance(identity(), Phase::RawReady).is_err());
        journal.created(identity().unwrap(), [3; 16]).unwrap();
        let mut other = identity().unwrap();
        other.index += 1;
        assert!(journal.advance(Some(other), Phase::Initializing).is_err());
        journal.advance(identity(), Phase::Initializing).unwrap();
        assert!(journal.advance(identity(), Phase::Created).is_err());
    }
    #[test]
    fn journal_filenames_cannot_escape_the_owned_directory() {
        for name in [
            "../other.vhdx",
            "C:\\other.vhdx",
            "cache-123.vhdx",
            "cache-123456789abcdef0123456789abcdef0.vhdx:stream",
        ] {
            let mut owner = record();
            owner.file = name.into();
            assert!(owner.validate().is_err());
        }
    }
}
