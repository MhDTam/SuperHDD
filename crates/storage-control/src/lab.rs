//! Explicit test identity. It never changes the direct-device production policy.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabIdentity {
    pub vm_id: String,
    pub run_id: String,
    pub source_parent_volume: String,
    pub cache_parent_volume: String,
}
pub(crate) fn guid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn volume(text: &str) -> bool {
    text.strip_prefix("\\\\?\\Volume{")
        .and_then(|s| s.strip_suffix("}\\"))
        .is_some_and(guid)
}
impl LabIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if !guid(&self.vm_id)
            || self.run_id.len() != 32
            || !self.run_id.bytes().all(|b| b.is_ascii_hexdigit())
            || !volume(&self.source_parent_volume)
            || !volume(&self.cache_parent_volume)
            || crate::same_volume(&self.source_parent_volume, &self.cache_parent_volume)
        {
            return Err("Invalid isolated-lab VM/run/volume identity.".into());
        }
        Ok(())
    }
}
/// Decode only the bounded Type-1 manufacturer/product from RawSMBIOSData.
/// https://learn.microsoft.com/windows/win32/api/sysinfoapi/nf-sysinfoapi-getsystemfirmwaretable
#[cfg(any(windows, test))]
pub(crate) fn smbios_identity(raw: &[u8]) -> Result<(String, String), String> {
    if raw.len() < 8
        || raw.len() > 1024 * 1024
        || u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize != raw.len() - 8
    {
        return Err("Invalid bounded firmware table length.".into());
    }
    let table = &raw[8..];
    let mut cursor = 0;
    let mut identity = None;
    while cursor < table.len() {
        let head = table
            .get(cursor..cursor + 4)
            .ok_or("Truncated firmware structure")?;
        let length = usize::from(head[1]);
        if length < 4 {
            return Err("Invalid firmware structure length.".into());
        }
        let body = table
            .get(cursor..cursor + length)
            .ok_or("Truncated firmware body")?;
        let strings = table
            .get(cursor + length..)
            .ok_or("Missing firmware strings")?;
        let end = strings
            .windows(2)
            .position(|p| p == [0, 0])
            .ok_or("Unterminated firmware strings")?;
        if head[0] == 1 {
            if length < 8 || identity.is_some() {
                return Err("Missing or duplicate system firmware identity.".into());
            }
            let strings: Vec<_> = strings[..end].split(|c| *c == 0).collect();
            let field = |index: u8| -> Result<String, String> {
                let index = index
                    .checked_sub(1)
                    .ok_or("Empty firmware identity index")?;
                let value = strings
                    .get(usize::from(index))
                    .ok_or("Missing firmware identity string")?;
                if value.is_empty() {
                    return Err("Empty firmware identity string.".into());
                }
                String::from_utf8(value.to_vec()).map_err(|e| e.to_string())
            };
            identity = Some((field(body[4])?, field(body[5])?));
        }
        cursor += length + end + 2;
        if head[0] == 127 {
            break;
        }
    }
    identity.ok_or_else(|| "No system firmware identity in SMBIOS.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn firmware_identity_is_bounded_and_rejects_truncated_or_duplicate_structures() {
        let mut raw = vec![0, 2, 8, 0, 0, 0, 0, 0];
        raw.extend_from_slice(&[1, 8, 0, 0, 1, 2, 0, 0]);
        raw.extend_from_slice(b"Microsoft Corporation\0Virtual Machine\0\0");
        let length = (raw.len() - 8) as u32;
        raw[4..8].copy_from_slice(&length.to_le_bytes());
        assert_eq!(
            smbios_identity(&raw).unwrap(),
            ("Microsoft Corporation".into(), "Virtual Machine".into())
        );
        for end in 0..raw.len() {
            assert!(smbios_identity(&raw[..end]).is_err());
        }
        let mut wrong = raw.clone();
        wrong[12] = 3;
        assert!(smbios_identity(&wrong).is_err());
        let body = raw[8..].to_vec();
        raw.extend_from_slice(&body);
        let length = (raw.len() - 8) as u32;
        raw[4..8].copy_from_slice(&length.to_le_bytes());
        assert!(smbios_identity(&raw).is_err());
    }
    pub(crate) fn identity() -> LabIdentity {
        LabIdentity {
            vm_id: "0150b344-482c-4758-b3e4-9f4bbf2ec015".into(),
            run_id: "123456789abcdef0123456789abcdef0".into(),
            source_parent_volume: "\\\\?\\Volume{00000000-0000-0000-0000-000000000001}\\".into(),
            cache_parent_volume: "\\\\?\\Volume{00000000-0000-0000-0000-000000000002}\\".into(),
        }
    }
    #[test]
    fn lab_identity_cannot_accept_paths_or_shared_parent_volumes() {
        let mut lab = identity();
        lab.validate().unwrap();
        lab.run_id = "..\\other".into();
        assert!(lab.validate().is_err());
        lab = identity();
        lab.cache_parent_volume = lab.source_parent_volume.clone();
        assert!(lab.validate().is_err());
        lab = identity();
        lab.vm_id = "not-a-vm-guid".into();
        assert!(lab.validate().is_err());
        lab = identity();
        lab.cache_parent_volume = "C:\\".into();
        assert!(lab.validate().is_err());
    }
}
