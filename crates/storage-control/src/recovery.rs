//! Durable service ownership, committed before the kernel can be configured.
//! The caller must protect and exclusively open the file; checksums detect
//! torn writes and are never treated as authorization from an untrusted user.
use crate::session::Target;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
const PAGE: usize = 4096;
const HEADER: usize = 64;
pub const BYTES: u64 = 8192;
const MAGIC: &[u8; 8] = b"SHDDRUN1";
pub fn config_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Prepared,
    Active,
    Stopped,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
    pub config_digest: [u8; 32],
    pub phase: Phase,
    pub target: Option<Target>,
    pub recoveries: u64,
}
impl Record {
    fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.config_digest == [0; 32]
            || (self.phase == Phase::Stopped) != self.target.is_none()
        {
            return Err("Invalid service ownership record.".into());
        }
        if let Some(target) = &self.target {
            if target.source_disk == target.cache_disk
                || target.container_instance == [0; 16]
                || target.capacity_bytes < 131072
                || target.capacity_bytes > crate::MAX_BUDGET_BYTES
                || !target.capacity_bytes.is_multiple_of(65536)
                || target.source_device == target.cache_device
                || target.source_volume == target.cache_volume
                || [
                    &target.source_device,
                    &target.cache_device,
                    &target.source_volume,
                    &target.cache_volume,
                ]
                .iter()
                .any(|v| v.is_empty() || v.len() > 520 || v.contains('\0'))
            {
                return Err("Invalid recorded service target.".into());
            }
        }
        Ok(())
    }
}
pub struct RunJournal {
    file: File,
    record: Record,
    sequence: u64,
    slot: usize,
}
impl RunJournal {
    pub fn create(file: File, digest: [u8; 32]) -> Result<Self, String> {
        if file.metadata().map_err(|e| e.to_string())?.len() != 0 {
            return Err("Refusing to replace service ownership.".into());
        }
        file.set_len(BYTES).map_err(|e| e.to_string())?;
        let record = Record {
            schema: 1,
            config_digest: digest,
            phase: Phase::Stopped,
            target: None,
            recoveries: 0,
        };
        let mut journal = Self {
            file,
            record: record.clone(),
            sequence: 0,
            slot: 1,
        };
        journal.write(record)?;
        Ok(journal)
    }
    pub fn open(mut file: File, digest: [u8; 32]) -> Result<Self, String> {
        if file.metadata().map_err(|e| e.to_string())?.len() != BYTES {
            return Err("Incomplete service ownership; preserving it.".into());
        }
        let mut bytes = [0u8; 8192];
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.read_exact(&mut bytes))
            .map_err(|e| e.to_string())?;
        let a = decode(&bytes[..PAGE]);
        let b = decode(&bytes[PAGE..]);
        let (slot, sequence, record) = match (a, b) {
            (Some((a, r)), Some((b, _))) if a > b => (0, a, r),
            (Some((a, _)), Some((b, r))) if b > a => (1, b, r),
            (Some((a, r)), None) => (0, a, r),
            (None, Some((b, r))) => (1, b, r),
            _ => return Err("Corrupt or ambiguous service ownership; preserving it.".into()),
        };
        if record.config_digest != digest {
            return Err(
                "Service configuration changed; previous ownership was not trusted.".into(),
            );
        }
        Ok(Self {
            file,
            record,
            sequence,
            slot,
        })
    }
    pub fn record(&self) -> &Record {
        &self.record
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn save(&mut self, phase: Phase, target: Option<Target>) -> Result<(), String> {
        let mut next = self.record.clone();
        next.phase = phase;
        next.target = target;
        self.write(next)
    }
    pub fn reconciled(&mut self) -> Result<(), String> {
        let mut next = self.record.clone();
        next.phase = Phase::Stopped;
        next.target = None;
        next.recoveries = next
            .recoveries
            .checked_add(1)
            .ok_or("Recovery count overflow")?;
        self.write(next)
    }
    fn write(&mut self, record: Record) -> Result<(), String> {
        record.validate()?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Service sequence overflow")?;
        let payload = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        if payload.len() > PAGE - HEADER {
            return Err("Service ownership exceeds one durable page.".into());
        }
        let mut page = [0u8; PAGE];
        page[..8].copy_from_slice(MAGIC);
        page[8..16].copy_from_slice(&sequence.to_le_bytes());
        page[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        page[HEADER..HEADER + payload.len()].copy_from_slice(&payload);
        let digest = checksum(&page);
        page[32..64].copy_from_slice(&digest);
        let slot = 1 - self.slot;
        self.file
            .seek(SeekFrom::Start((slot * PAGE) as u64))
            .and_then(|_| self.file.write_all(&page))
            .and_then(|_| self.file.sync_all())
            .map_err(|e| e.to_string())?;
        self.sequence = sequence;
        self.slot = slot;
        self.record = record;
        Ok(())
    }
}
fn checksum(page: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(&page[..32]);
    h.update(&page[64..]);
    h.finalize().into()
}
fn decode(page: &[u8]) -> Option<(u64, Record)> {
    if page.len() != PAGE
        || &page[..8] != MAGIC
        || page[20..32].iter().any(|b| *b != 0)
        || page[32..64] != checksum(page)
    {
        return None;
    }
    let sequence = u64::from_le_bytes(page[8..16].try_into().ok()?);
    let length = u32::from_le_bytes(page[16..20].try_into().ok()?) as usize;
    if sequence == 0
        || length == 0
        || length > PAGE - HEADER
        || page[HEADER + length..].iter().any(|b| *b != 0)
    {
        return None;
    }
    let record: Record = serde_json::from_slice(&page[HEADER..HEADER + length]).ok()?;
    record.validate().ok()?;
    Some((sequence, record))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            source_device: "source".into(),
            cache_device: "cache".into(),
            source_disk: 1,
            cache_disk: 2,
            capacity_bytes: 1048576,
            source_volume: "source-guid".into(),
            cache_volume: "cache-guid".into(),
            container_instance: [9; 16],
        }
    }
    #[test]
    fn prepared_target_is_durable_and_bound_to_the_exact_configuration() {
        let file = tempfile::tempfile().unwrap();
        let copy = file.try_clone().unwrap();
        let mut j = RunJournal::create(file, [1; 32]).unwrap();
        j.save(Phase::Prepared, Some(target())).unwrap();
        drop(j);
        let mut j = RunJournal::open(copy, [1; 32]).unwrap();
        assert_eq!(j.record().phase, Phase::Prepared);
        assert_eq!(j.record().target, Some(target()));
        j.reconciled().unwrap();
        assert_eq!(j.record().recoveries, 1);
        assert!(j.record().target.is_none());
        let copy = j.file.try_clone().unwrap();
        drop(j);
        assert!(RunJournal::open(copy, [2; 32]).is_err());
    }
    #[test]
    fn torn_latest_record_preserves_the_prior_committed_target() {
        let file = tempfile::tempfile().unwrap();
        let mut j = RunJournal::create(file, [1; 32]).unwrap();
        j.save(Phase::Prepared, Some(target())).unwrap();
        j.save(Phase::Active, Some(target())).unwrap();
        let mut f = j.file.try_clone().unwrap();
        f.seek(SeekFrom::Start((j.slot * PAGE) as u64)).unwrap();
        f.write_all(&[0x77; 100]).unwrap();
        f.sync_all().unwrap();
        drop(j);
        let j = RunJournal::open(f, [1; 32]).unwrap();
        assert_eq!(j.record().phase, Phase::Prepared);
        assert_eq!(j.record().target, Some(target()));
    }
    #[test]
    fn corrupt_journal_and_overlapping_target_cannot_authorize_recovery() {
        let f = tempfile::tempfile().unwrap();
        f.set_len(BYTES).unwrap();
        assert!(RunJournal::open(f, [1; 32]).is_err());
        let f = tempfile::tempfile().unwrap();
        let mut j = RunJournal::create(f, [1; 32]).unwrap();
        let mut t = target();
        t.cache_disk = t.source_disk;
        assert!(j.save(Phase::Active, Some(t)).is_err());
        assert_eq!(j.sequence(), 1);
    }
}
