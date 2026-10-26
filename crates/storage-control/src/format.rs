//! Shared v4 RAW-store bootstrap and ownership validation; never performs I/O.
const MAGIC: &[u8; 8] = b"SHDDRAW4";
/// Same layout as ShddStoreLayout in raw_store.h, including sector rounding.
pub fn layout(budget: u64) -> Result<(u64, u64), String> {
    if !(131072..=crate::MAX_BUDGET_BYTES).contains(&budget) || !budget.is_multiple_of(4096) {
        return Err("Invalid RAW store budget.".into());
    }
    let mut slots = (budget - 4096) / (65536 + 64);
    while slots > 0 {
        let metadata = (slots * 64 + 4095) & !4095;
        let total = 4096 + metadata + slots * 65536;
        if total <= budget {
            return Ok((slots * 65536, total));
        }
        slots -= 1;
    }
    Err("RAW store has no payload capacity.".into())
}
fn hash(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .enumerate()
        .fold(0xcbf29ce484222325u64, |hash, (i, byte)| {
            (hash
                ^ if (72..80).contains(&i) {
                    0
                } else {
                    u64::from(*byte)
                })
            .wrapping_mul(0x100000001b3)
        })
}
pub fn bootstrap(instance: [u8; 16]) -> Result<[u8; 4096], String> {
    if instance == [0; 16] {
        return Err("A nonzero cache instance ID is required.".into());
    }
    let mut bytes = [0u8; 4096];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&4096u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&64u32.to_le_bytes());
    bytes[52..56].copy_from_slice(&1u32.to_le_bytes());
    bytes[56..72].copy_from_slice(&instance);
    let checksum = hash(&bytes[..128]);
    bytes[72..80].copy_from_slice(&checksum.to_le_bytes());
    Ok(bytes)
}
/// Accept an owned pristine bootstrap or a structurally valid populated store.
/// This does not authorize payload hits; the kernel still revalidates source bytes.
pub fn validate_header(
    bytes: &[u8; 4096],
    instance: [u8; 16],
    capacity: u64,
) -> Result<(), String> {
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let u64_at = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
    if instance == [0; 16]
        || &bytes[..8] != MAGIC
        || u32_at(8) != 4
        || u32_at(12) != 4096
        || u32_at(28) != 64
        || bytes[56..72] != instance
        || u64_at(72) != hash(&bytes[..128])
        || bytes[80..128].iter().any(|b| *b != 0)
    {
        return Err("Unrecognized or corrupted owned cache header.".into());
    }
    match u32_at(52) {
        1 if bytes == &bootstrap(instance)? => Ok(()),
        2 => {
            let slots = u64::from(u32_at(24));
            let metadata = (slots * 64 + 4095) & !4095;
            let total = 4096 + metadata + slots * 65536;
            if slots == 0
                || u64_at(40) < 65536
                || u64_at(40) > i64::MAX as u64
                || u64_at(32) != 4096 + metadata
                || u64_at(16) != total
                || total > capacity
                || total > crate::MAX_BUDGET_BYTES
            {
                return Err("Invalid populated cache layout.".into());
            }
            Ok(())
        }
        _ => Err("Invalid cache header state.".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_matches_native_small_and_runtime_vectors_without_overflow() {
        assert_eq!(layout(131072).unwrap(), (65536, 73728));
        assert_eq!(layout(262144).unwrap(), (196608, 204800));
        assert_eq!(layout(190 * 1024 * 1024).unwrap(), (198967296, 199168000));
        let budget = crate::MAX_BUDGET_BYTES & !4095;
        let (payload, total) = layout(budget).unwrap();
        assert!(payload < total && total <= budget);
        assert_eq!(total % 4096, 0);
        assert!(layout(4096).is_err());
        assert!(layout(u64::MAX).is_err());
    }
    #[test]
    fn corrupted_or_foreign_markers_are_never_reinitialized() {
        let marker = bootstrap([7; 16]).unwrap();
        validate_header(&marker, [7; 16], 1048576).unwrap();
        assert!(validate_header(&marker, [8; 16], 1048576).is_err());
        for i in [0, 8, 12, 24, 28, 52, 56, 72, 80, 127] {
            let mut damaged = marker;
            damaged[i] ^= 1;
            assert!(validate_header(&damaged, [7; 16], 1048576).is_err());
        }
    }
    #[test]
    fn populated_layout_is_bounded_by_the_actual_volume() {
        let mut marker = bootstrap([9; 16]).unwrap();
        marker[16..24].copy_from_slice(&73728u64.to_le_bytes());
        marker[24..28].copy_from_slice(&1u32.to_le_bytes());
        marker[32..40].copy_from_slice(&8192u64.to_le_bytes());
        marker[40..48].copy_from_slice(&1048576u64.to_le_bytes());
        marker[52..56].copy_from_slice(&2u32.to_le_bytes());
        let checksum = hash(&marker[..128]);
        marker[72..80].copy_from_slice(&checksum.to_le_bytes());
        assert!(validate_header(&marker, [9; 16], 73728).is_ok());
        assert!(validate_header(&marker, [9; 16], 65536).is_err());
    }
}
