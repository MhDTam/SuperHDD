use serde::{Deserialize, Serialize};
use std::{
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
#[cfg(windows)]
pub mod app_library;
#[cfg(windows)]
pub mod cache_benchmark;
pub mod cache_runtime;
#[cfg(windows)]
pub mod direct_cache;
pub mod direct_driver;
pub mod downloads;
pub mod driver;
#[cfg(windows)]
pub mod folder_runtime;
pub mod instance;
#[cfg(windows)]
pub mod ipc;
#[cfg(windows)]
pub mod loopback_cache;
#[cfg(windows)]
pub mod managed_cache;
#[cfg(windows)]
pub mod native_package;
#[cfg(windows)]
pub mod native_runtime;
#[cfg(windows)]
pub mod paging_cache;
pub mod paging_driver;
pub mod paging_runtime;
pub mod pnp_lab;
pub mod raw_lab;
#[cfg(windows)]
pub mod transparent_cache;
pub mod transparent_driver;
pub mod transparent_runtime;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub letter: String,
    pub label: String,
    pub file_system: String,
    pub size: u64,
    pub free: u64,
    pub partition_number: u32,
    pub system: bool,
    pub health: String,
    pub encryption: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub number: u32,
    pub id: String,
    pub model: String,
    pub serial: String,
    pub kind: String,
    #[serde(default)]
    pub media_evidence: String,
    pub bus: String,
    pub size: u64,
    pub online: bool,
    pub read_only: bool,
    pub system: bool,
    pub volumes: Vec<Volume>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Driver {
    pub name: String,
    pub state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub disks: Vec<Disk>,
    pub drivers: Vec<Driver>,
    pub service_state: String,
    pub collected_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheRequest {
    pub source_disk_id: String,
    pub source_letter: String,
    pub ssd_disk_id: String,
    pub ssd_letter: String,
    pub capacity_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<CacheScope>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum CacheScope {
    Selected { folders: Vec<String> },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePlan {
    pub request: CacheRequest,
    pub reserve_bytes: u64,
    pub remaining_bytes: u64,
    pub recommended_bytes: u64,
    pub activation_available: bool,
    pub reasons: Vec<String>,
}

pub fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.stdin(Stdio::null());
    command
}

pub fn discover() -> Result<Snapshot, String> {
    #[cfg(not(windows))]
    {
        return Err("Storage discovery requires Windows.".into());
    }
    #[cfg(windows)]
    {
        // Fixed, bundled script. No user-controlled PowerShell interpolation.
        let mut child = hidden_command("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                include_str!("discover.ps1"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        use std::io::Read;
        let mut stdout = child.stdout.take().ok_or("Discovery stdout unavailable")?;
        let mut stderr = child.stderr.take().ok_or("Discovery stderr unavailable")?;
        let output_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let error_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(25);
        let status = loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(status) => break status,
                None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = output_reader.join();
                    let _ = error_reader.join();
                    return Err("Storage discovery timed out; retry diagnostics.".into());
                }
            }
        };
        let stdout = output_reader
            .join()
            .map_err(|_| "Discovery output reader failed")?
            .map_err(|e| e.to_string())?;
        let stderr = error_reader
            .join()
            .map_err(|_| "Discovery error reader failed")?
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(String::from_utf8_lossy(&stderr).trim().to_string());
        }
        let mut snapshot: Snapshot = serde_json::from_slice(&stdout)
            .map_err(|e| format!("Invalid discovery response: {e}"))?;
        for disk in &mut snapshot.disks {
            disk.media_evidence = "Windows storage media type".into();
            if disk.kind == "Unknown" {
                if let Some(seeks) = seek_penalty(disk.number) {
                    disk.kind = if seeks { "HDD" } else { "SSD" }.into();
                    disk.media_evidence = "Device seek-penalty descriptor".into();
                } else if let Some(rpm) = ata_rotation_rate(disk.number) {
                    disk.kind = if rpm == 1 { "SSD" } else { "HDD" }.into();
                    disk.media_evidence = format!("ATA IDENTIFY rotation rate: {rpm}");
                } else if catalog_media_type(&disk.model).is_some() {
                    disk.kind = "HDD".into();
                    disk.media_evidence = "WD AV-GP WD10EURX manufacturer model catalog".into();
                } else {
                    disk.media_evidence =
                        "Device did not report a supported media descriptor".into();
                }
            }
        }
        Ok(snapshot)
    }
}

// Narrow model-family fallback, not a heuristic based on SATA bus or transfer speed.
// WD's AV-GP product documentation identifies WD10EURX as a rotational 3.5-inch HDD.
fn catalog_media_type(model: &str) -> Option<&'static str> {
    let model = model.trim().strip_prefix("WDC ").unwrap_or(model.trim());
    (model.split('-').next() == Some("WD10EURX")).then_some("HDD")
}

#[cfg(windows)]
fn ata_rotation_rate(number: u32) -> Option<u16> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };
    let path: Vec<u16> = format!("\\\\.\\PhysicalDrive{number}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // StorageDeviceProtocolSpecificProperty / ProtocolTypeAta / AtaDataTypeIdentify.
    let mut query = [0u32; 12];
    query[0] = 50;
    query[2] = 2;
    query[3] = 1;
    query[6] = 40;
    query[7] = 512;
    let mut output = [0u8; 560];
    let mut returned = 0;
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let ok = DeviceIoControl(
            handle,
            0x002d1400,
            query.as_ptr().cast(),
            std::mem::size_of_val(&query) as u32,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        ) != 0;
        CloseHandle(handle);
        if !ok || returned < 48 {
            return None;
        }
    }
    let offset = u32::from_le_bytes(output[24..28].try_into().ok()?) as usize;
    let length = u32::from_le_bytes(output[28..32].try_into().ok()?) as usize;
    let start = 8usize.checked_add(offset)?;
    if offset < 40
        || length < 512
        || start.checked_add(512)? > returned as usize
        || start.checked_add(512)? > output.len()
    {
        return None;
    }
    let rpm = u16::from_le_bytes(output[start + 434..start + 436].try_into().ok()?);
    (rpm == 1 || (1025..=65534).contains(&rpm)).then_some(rpm)
}

#[cfg(windows)]
fn seek_penalty(number: u32) -> Option<bool> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };
    let path: Vec<u16> = format!("\\\\.\\PhysicalDrive{number}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let query = [7u32, 0u32, 0u32];
    let mut descriptor = [0u8; 12];
    let mut returned = 0;
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let ok = DeviceIoControl(
            handle,
            0x002d1400,
            query.as_ptr().cast(),
            std::mem::size_of_val(&query) as u32,
            descriptor.as_mut_ptr().cast(),
            descriptor.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        ) != 0;
        CloseHandle(handle);
        if ok && returned >= 9 {
            Some(descriptor[8] != 0)
        } else {
            None
        }
    }
}

pub struct DiscoveryCache(Mutex<Option<(Instant, Snapshot)>>);
impl Default for DiscoveryCache {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}
impl DiscoveryCache {
    pub fn get(&self, force: bool) -> Result<Snapshot, String> {
        let mut guard = self.0.lock().map_err(|_| "Discovery lock unavailable")?;
        if !force {
            if let Some((time, snapshot)) = guard.as_ref() {
                if time.elapsed() < Duration::from_secs(30) {
                    return Ok(snapshot.clone());
                }
            }
        }
        let snapshot = discover()?;
        *guard = Some((Instant::now(), snapshot.clone()));
        Ok(snapshot)
    }
}

pub fn plan_cache(snapshot: &Snapshot, request: CacheRequest) -> Result<CachePlan, String> {
    let source = snapshot
        .disks
        .iter()
        .find(|d| d.id == request.source_disk_id)
        .ok_or("Source disk no longer exists.")?;
    let ssd = snapshot
        .disks
        .iter()
        .find(|d| d.id == request.ssd_disk_id)
        .ok_or("SSD no longer exists.")?;
    if source.id == ssd.id {
        return Err("Source and cache must be separate physical devices.".into());
    }
    if source.kind != "HDD" || ssd.kind != "SSD" {
        return Err("A positively identified HDD and SSD are required.".into());
    }
    if !source.online || !ssd.online || source.read_only || ssd.read_only {
        return Err("Both devices must be online and writable.".into());
    }
    let volume = source
        .volumes
        .iter()
        .find(|v| v.letter == request.source_letter)
        .ok_or("Source volume no longer exists.")?;
    let destination = ssd
        .volumes
        .iter()
        .find(|v| v.letter == request.ssd_letter)
        .ok_or("SSD volume no longer exists.")?;
    if volume.system
        || destination.system
        || volume.file_system != "NTFS"
        || destination.file_system != "NTFS"
    {
        return Err("Only NTFS data volumes on the source HDD and cache SSD are supported.".into());
    }
    if volume.health != "Healthy" || destination.health != "Healthy" {
        return Err("Both volumes must report Healthy.".into());
    }
    let reserve = (destination.size / 10).max(10 * 1024u64.pow(3));
    let max = destination
        .free
        .saturating_sub(reserve)
        .min(superhdd_storage_control::MAX_BUDGET_BYTES);
    if request.capacity_bytes < 1024u64.pow(3)
        || request.capacity_bytes > max
        || !request.capacity_bytes.is_multiple_of(65536)
    {
        return Err(format!("Cache capacity must be aligned to 64 KiB, at least 1 GiB, and at most {max} bytes after the SSD reserve."));
    }
    let mut reasons = vec![];
    if request.scope.is_some() {
        reasons.push("selectedScopeUnavailable".into());
    }
    if !snapshot
        .drivers
        .iter()
        .any(|d| d.name == "SuperHDDCacheDriver" && d.state == "Running")
    {
        reasons.push("driverNotLoaded".into())
    }
    if volume.encryption != "FullyDecrypted" {
        reasons.push("encryptionUnknown".into())
    }
    // Release gate: local compilation is not equivalent to a qualified storage driver.
    #[cfg(windows)]
    let native_ready = paging_runtime::available();
    #[cfg(not(windows))]
    let native_ready = false;
    if cache_runtime::qualified_driver_hash().is_none() && !native_ready {
        reasons.push("runtimeReleasePending".into());
    }
    Ok(CachePlan {
        remaining_bytes: destination.free - request.capacity_bytes,
        reserve_bytes: reserve,
        recommended_bytes: (max.min(source.size / 10) / 65536) * 65536,
        activation_available: reasons.iter().all(|r| r == "driverNotLoaded"),
        reasons,
        request,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manufacturer_fallback_is_narrow_and_never_guesses_from_bus() {
        assert_eq!(catalog_media_type("WDC WD10EURX-63FH1Y0"), Some("HDD"));
        assert_eq!(catalog_media_type("WD10EURX-14UY4Y0"), Some("HDD"));
        assert_eq!(catalog_media_type("Samsung SSD 980 PRO 1TB"), None);
        assert_eq!(catalog_media_type("CT1000MX500SSD1"), None);
        assert_eq!(catalog_media_type("WD10EURXX-unknown"), None);
        assert_eq!(catalog_media_type("SATA drive"), None);
    }
    #[test]
    fn data_partition_on_boot_disk_is_not_a_system_volume() {
        let mut data = snapshot();
        data.disks[0].system = true;
        assert!(plan_cache(&data, request()).is_ok());
    }
    fn snapshot() -> Snapshot {
        serde_json::from_str(r#"{"disks":[{"number":1,"id":"hdd","model":"test HDD","serial":"h","kind":"HDD","bus":"SATA","size":1099511627776,"online":true,"readOnly":false,"system":false,"volumes":[{"letter":"D:","label":"test","fileSystem":"NTFS","size":1099511627776,"free":500000000000,"partitionNumber":1,"system":false,"health":"Healthy","encryption":"FullyDecrypted"}]},{"number":2,"id":"ssd","model":"test SSD","serial":"s","kind":"SSD","bus":"NVMe","size":1099511627776,"online":true,"readOnly":false,"system":false,"volumes":[{"letter":"M:","label":"test","fileSystem":"NTFS","size":1099511627776,"free":400000000000,"partitionNumber":1,"system":false,"health":"Healthy","encryption":"FullyDecrypted"}]}],"drivers":[],"serviceState":"NotInstalled","primoPath":null,"collectedAt":"test"}"#).unwrap()
    }
    fn request() -> CacheRequest {
        CacheRequest {
            source_disk_id: "hdd".into(),
            source_letter: "D:".into(),
            ssd_disk_id: "ssd".into(),
            ssd_letter: "M:".into(),
            capacity_bytes: 20 * 1024u64.pow(3),
            scope: None,
        }
    }
    #[test]
    fn valid_manual_capacity_is_preserved() {
        let p = plan_cache(&snapshot(), request()).unwrap();
        assert_eq!(p.request.capacity_bytes, 20 * 1024u64.pow(3));
        assert!(!p.activation_available);
    }
    #[test]
    fn selected_scope_is_preserved_and_never_runs_as_a_volume_cache() {
        let mut r = request();
        r.scope = Some(CacheScope::Selected {
            folders: vec![r"D:\Games\Example".into()],
        });
        let plan = plan_cache(&snapshot(), r.clone()).unwrap();
        assert!(!plan.activation_available);
        assert!(plan.reasons.contains(&"selectedScopeUnavailable".into()));
        let serialized = serde_json::to_value(&plan.request).unwrap();
        assert_eq!(serialized["scope"]["folders"][0], r"D:\Games\Example");
        let command = cache_runtime::Command {
            operation: cache_runtime::Operation::Start,
            request: Some(r),
        };
        assert_eq!(command.validate().unwrap_err(), "selectedScopeUnavailable");
        assert!(command.encode().is_err());
        // Malformed or unknown policies must not deserialize as whole-volume.
        let mut unknown = serialized.clone();
        unknown["scope"]["mode"] = serde_json::json!("unknown");
        assert!(serde_json::from_value::<CacheRequest>(unknown).is_err());
        let mut unknown_field = serialized;
        unknown_field["scope"]["allowEverything"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CacheRequest>(unknown_field).is_err());
    }
    #[test]
    fn planned_budget_never_exceeds_the_native_decimal_limit() {
        let mut r = request();
        r.capacity_bytes = 25 * 1024u64.pow(3);
        assert!(plan_cache(&snapshot(), r).is_err());
        assert!(
            plan_cache(&snapshot(), request())
                .unwrap()
                .recommended_bytes
                <= superhdd_storage_control::MAX_BUDGET_BYTES
        );
    }
    #[test]
    fn rejects_same_device() {
        let mut r = request();
        r.ssd_disk_id = "hdd".into();
        assert!(plan_cache(&snapshot(), r).is_err())
    }
    #[test]
    fn rejects_system_source() {
        let mut s = snapshot();
        s.disks[0].volumes[0].system = true;
        assert!(plan_cache(&s, request()).is_err())
    }
    #[test]
    fn rejects_system_cache_volume_before_elevation() {
        let mut s = snapshot();
        s.disks[1].volumes[0].system = true;
        assert!(plan_cache(&s, request()).is_err());
    }
    #[test]
    fn protects_free_space() {
        let mut r = request();
        r.capacity_bytes = 500 * 1024u64.pow(3);
        assert!(plan_cache(&snapshot(), r).is_err())
    }
    #[test]
    fn unknown_encryption_is_never_silently_accepted() {
        let mut s = snapshot();
        s.disks[0].volumes[0].encryption = "Unknown".into();
        assert!(plan_cache(&s, request())
            .unwrap()
            .reasons
            .iter()
            .any(|r| r.contains("encryption")))
    }
}
