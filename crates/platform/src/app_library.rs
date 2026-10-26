//! Selected-folder mounts sharing an LRU loading-priority cache per configuration.
use crate::{CacheRequest, CacheScope, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
const BLOCK: u64 = 65536;
const MIN: u64 = 1024 * 1024 * 1024;
const MAX: u64 = 25_000_000_000 / BLOCK * BLOCK;
#[path = "app_library_data.rs"]
mod data_scope;
type DataTargets = BTreeMap<String, (Instant, Option<String>)>;
static DATA_TARGETS: Mutex<DataTargets> = Mutex::new(BTreeMap::new());
static MUTATION: Mutex<()> = Mutex::new(());
static DISCOVERY: Mutex<Option<(Instant, Vec<AppEntry>)>> = Mutex::new(None);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub origin: String,
    pub kind: String,
    #[serde(default)]
    pub icon_data_uri: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Discovered {
    name: String,
    path: String,
    origin: String,
    kind: String,
    icon_data_uri: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheConfiguration {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub source_disk_id: String,
    pub source_letter: String,
    pub ssd_disk_id: String,
    pub ssd_letter: String,
    pub budget_bytes: u64,
    #[serde(default)]
    pub source_volume_id: String,
    #[serde(default)]
    pub ssd_volume_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    budget_bytes: u64,
    apps: Vec<AppEntry>,
    #[serde(default)]
    version: u32,
    #[serde(default)]
    configurations: Vec<CacheConfiguration>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    cache_modes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    data_links: BTreeMap<String, String>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            budget_bytes: MAX,
            apps: vec![],
            version: 2,
            configurations: vec![],
            cache_modes: BTreeMap::new(),
            data_links: BTreeMap::new(),
        }
    }
}
pub fn workspace() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_default();
    PathBuf::from(p.to_string_lossy().trim_start_matches("\\\\?\\"))
}
fn root() -> PathBuf {
    workspace().join("artifacts/library-runtime")
}
fn read(path: impl AsRef<Path>) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
fn valid_letter(p: &str) -> bool {
    p.len() == 2 && p.as_bytes()[0].is_ascii_uppercase() && p.as_bytes()[1] == b':'
}
fn volume_identity(letter: &str) -> Result<String, String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
    if !valid_letter(letter) {
        return Err("configInvalid".into());
    }
    let path: Vec<u16> = format!("{}\\", letter)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut value = [0u16; 128];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(path.as_ptr(), value.as_mut_ptr(), value.len() as u32)
    } == 0
    {
        return Err("configDeviceMissing".into());
    }
    Ok(String::from_utf16_lossy(
        &value[..value.iter().position(|n| *n == 0).unwrap_or(value.len())],
    ))
}
fn configuration_id(c: &CacheConfiguration) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            format!("configuration|{}|{}", c.source_disk_id, c.source_letter).as_bytes()
        )
    )[..32]
        .into()
}
fn load() -> Result<Store, String> {
    let p = root().join("library.json");
    if !p.exists() {
        return Ok(Store::default());
    }
    let mut s: Store = serde_json::from_slice(&fs::read(p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if s.version < 2 {
        for a in &s.apps {
            if let Some(v) = read(root().join(&a.id).join("request.json")) {
                if let Ok(r) = serde_json::from_value::<CacheRequest>(v) {
                    if !s
                        .configurations
                        .iter()
                        .any(|c| c.source_letter == r.source_letter)
                    {
                        let mut c = CacheConfiguration {
                            id: String::new(),
                            name: format!("{} → {}", r.source_letter, r.ssd_letter),
                            source_disk_id: r.source_disk_id,
                            source_letter: r.source_letter,
                            ssd_disk_id: r.ssd_disk_id,
                            ssd_letter: r.ssd_letter,
                            budget_bytes: s.budget_bytes,
                            source_volume_id: String::new(),
                            ssd_volume_id: String::new(),
                        };
                        c.id = configuration_id(&c);
                        c.source_volume_id = volume_identity(&c.source_letter).unwrap_or_default();
                        c.ssd_volume_id = volume_identity(&c.ssd_letter).unwrap_or_default();
                        s.configurations.push(c);
                    }
                }
            }
        }
        s.version = 2;
    }
    if s.configurations.len() > 8
        || s.configurations.iter().any(|c| {
            !valid_letter(&c.source_letter)
                || !valid_letter(&c.ssd_letter)
                || !(MIN..=MAX).contains(&c.budget_bytes)
                || !valid_id(&c.id)
        })
    {
        return Err("configInvalid".into());
    }
    Ok(s)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    let a: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            a.as_ptr(),
            b.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
fn save(s: &Store) -> Result<(), String> {
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    atomic(
        &root().join("library.json"),
        &serde_json::to_vec(s).map_err(|e| e.to_string())?,
    )
}
fn lock() -> Result<fs::File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(root().join("mutation.lock"))
        .map_err(|_| "libraryBusy".into())
}
pub fn normalize(path: &str) -> Result<String, String> {
    let p = path
        .trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_owned();
    if p.len() < 4
        || p.len() > 2048
        || !p.as_bytes()[0].is_ascii_alphabetic()
        || p.as_bytes()[1] != b':'
        || p.as_bytes()[2] != b'\\'
        || p.chars().any(char::is_control)
        || p[3..].contains([':', '"', '<', '>', '|', '?', '*'])
        || p.split('\\')
            .skip(1)
            .any(|s| s.is_empty() || s == "." || s == ".." || s.ends_with(['.', ' ']))
    {
        return Err("libraryInvalidFolder".into());
    }
    let mut out = p;
    out.replace_range(..1, &out[..1].to_ascii_uppercase());
    Ok(out)
}
pub fn app_id(path: &str) -> String {
    format!("{:x}", Sha256::digest(path.to_lowercase().as_bytes()))[..32].into()
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn overlaps(a: &str, b: &str) -> bool {
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\"))
}
fn dir(id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid app ID".into());
    }
    Ok(root().join(id))
}
fn journal(id: &str) -> Option<Value> {
    read(dir(id).ok()?.join("current.json"))
}
fn task(j: &Value, id: &str) -> Result<PathBuf, String> {
    let n = j["id"].as_str().ok_or("Invalid session ID")?;
    if !valid_id(n) {
        return Err("Invalid session ID".into());
    }
    let expected = dir(id)?.join(n);
    if Path::new(j["task"].as_str().ok_or("Missing session path")?) != expected {
        return Err("Session path mismatch".into());
    }
    Ok(expected)
}
fn raw(id: &str) -> Option<Value> {
    let j = journal(id)?;
    read(task(&j, id).ok()?.join("status.json"))
}
fn cache_file(j: &Value, id: &str) -> Option<PathBuf> {
    if let Some(cache) = j["cache"].as_str() {
        let drive = j["cacheDrive"].as_str()?;
        let owner = j["cacheOwner"].as_str()?;
        if !valid_letter(drive)
            || owner.len() != 16
            || !owner.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        let t = task(j, id).ok()?;
        let n = t.file_name()?.to_str()?;
        let expected = if j["sharedPool"] == true {
            let config = j["configurationId"].as_str()?;
            if !valid_id(config) {
                return None;
            }
            format!(
                "{}\\SuperHDD-Cache\\{}\\pools\\{}\\cache.bin",
                drive, owner, config
            )
        } else {
            format!(
                "{}\\SuperHDD-Cache\\{}\\{}\\{}\\cache.bin",
                drive, owner, id, n
            )
        };
        if !cache.eq_ignore_ascii_case(&expected) {
            return None;
        }
        Some(PathBuf::from(cache))
    } else {
        Some(task(j, id).ok()?.join("cache.bin"))
    }
}
fn phase(id: &str) -> String {
    let Some(j) = journal(id) else {
        return "stopped".into();
    };
    if j["restored"] == true {
        return "stopped".into();
    }
    if !crate::folder_runtime::alive(&j, "supervisor") {
        return "faulted".into();
    }
    if j["phase"] == "active" && crate::folder_runtime::alive(&j, "worker") {
        return if raw(id).is_some_and(|n| n["paused"] == true) {
            "paused"
        } else {
            "active"
        }
        .into();
    }
    "starting".into()
}
fn active(s: &Store) -> Vec<AppEntry> {
    s.apps
        .iter()
        .filter(|a| phase(&a.id) != "stopped")
        .cloned()
        .collect()
}
fn on_source(a: &AppEntry, c: &CacheConfiguration) -> bool {
    a.path.starts_with(&(c.source_letter.clone() + "\\"))
}
fn active_for(s: &Store, c: &CacheConfiguration) -> Vec<AppEntry> {
    active(s).into_iter().filter(|a| on_source(a, c)).collect()
}
fn allocation(id: &str) -> u64 {
    let Some(j) = journal(id) else { return 0 };
    if j["restored"] == true {
        return 0;
    }
    cache_file(&j, id)
        .and_then(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .unwrap_or_else(|| {
            raw(id)
                .and_then(|v| v["capacityBytes"].as_u64())
                .unwrap_or(j["capacityBytes"].as_u64().unwrap_or(0))
        })
}
fn allocated(items: &[AppEntry]) -> u64 {
    let mut pools = BTreeMap::new();
    for a in items {
        if let Some(j) = journal(&a.id) {
            if j["restored"] != true {
                if let Some(path) = cache_file(&j, &a.id) {
                    pools.insert(path, allocation(&a.id));
                }
            }
        }
    }
    pools.values().sum()
}
fn physical_allocated(items: &[AppEntry]) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::GetLastError, Storage::FileSystem::GetCompressedFileSizeW,
    };
    let mut paths = std::collections::BTreeSet::new();
    for a in items {
        if let Some(j) = journal(&a.id) {
            // A shared pool keeps its file (and retained data) while stopped.
            if j["restored"] != true || j["sharedPool"] == true {
                if let Some(path) = cache_file(&j, &a.id) {
                    if j["sharedPool"] == true {
                        paths.insert(PathBuf::from(format!("{}.index", path.display())));
                    }
                    paths.insert(path);
                }
            }
        }
    }
    paths
        .iter()
        .map(|p| {
            let name: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut high = 0;
            let low = unsafe { GetCompressedFileSizeW(name.as_ptr(), &mut high) };
            if low == u32::MAX && unsafe { GetLastError() } != 0 {
                0
            } else {
                (u64::from(high) << 32) | u64::from(low)
            }
        })
        .sum()
}
fn resident(items: &[AppEntry]) -> u64 {
    let mut pools: BTreeMap<PathBuf, (u64, u64)> = BTreeMap::new();
    let mut legacy = 0;
    for a in items {
        if let (Some(j), Some(n)) = (journal(&a.id), raw(&a.id)) {
            if j["restored"] == true {
                continue;
            }
            if j["sharedPool"] == true {
                if let Some(path) = cache_file(&j, &a.id) {
                    let at = n["sampledAt"].as_u64().unwrap_or(0);
                    let bytes = n["poolOccupiedBytes"].as_u64().unwrap_or(0);
                    let old = pools.entry(path).or_insert((0, 0));
                    if at >= old.0 {
                        *old = (at, bytes);
                    }
                }
            } else {
                legacy += n["occupiedBytes"].as_u64().unwrap_or(0);
            }
        }
    }
    legacy + pools.values().map(|(_, bytes)| bytes).sum::<u64>()
}
fn hash(p: PathBuf) -> Option<String> {
    Some(format!("{:X}", Sha256::digest(fs::read(p).ok()?)))
}
pub fn available() -> bool {
    let w = workspace();
    let Some(q) = read(w.join("artifacts/loading-cache/qualified.json")) else {
        return false;
    };
    q["verified"] == true
        && hash(w.join("artifacts/loading-cache/folder-worker.exe")).as_deref()
            == q["workerSha256"].as_str()
        && hash(w.join("tools/loading-cache/session.ps1")).as_deref() == q["sessionSha256"].as_str()
        && hash(w.join("tools/loading-cache/schedule-session.ps1")).as_deref()
            == q["launcherSha256"].as_str()
}
fn legacy_active() -> bool {
    matches!(
        crate::folder_runtime::state()["phase"].as_str(),
        Some("active" | "paused" | "starting" | "faulted")
    )
}
pub fn discover() -> Result<Vec<AppEntry>, String> {
    let script = format!(
        "{}\n{}",
        include_str!("library-icons.ps1"),
        include_str!("installed_apps.ps1")
    );
    let mut child = crate::hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env(
            "SUPERHDD_LIBRARY_PROFILES",
            serde_json::to_string(
                &load()?
                    .apps
                    .iter()
                    .map(|a| json!({"name":a.name,"path":a.path}))
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| e.to_string())?,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut out = child.stdout.take().ok_or("Missing output")?;
    let mut err = child.stderr.take().ok_or("Missing errors")?;
    let output = thread::spawn(move || {
        let mut b = vec![];
        out.read_to_end(&mut b).map(|_| b)
    });
    let errors = thread::spawn(move || {
        let mut b = vec![];
        err.read_to_end(&mut b).map(|_| b)
    });
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("libraryDiscoveryTimeout".into());
        }
        thread::sleep(Duration::from_millis(100))
    };
    let bytes = output
        .join()
        .map_err(|_| "Discovery reader")?
        .map_err(|e| e.to_string())?;
    let errors = errors
        .join()
        .map_err(|_| "Discovery reader")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(String::from_utf8_lossy(&errors).into());
    }
    let items: Vec<Discovered> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .filter_map(|a| {
            let path = normalize(&a.path).ok()?;
            Some(AppEntry {
                id: app_id(&path),
                name: a.name,
                path,
                origin: a.origin,
                kind: a.kind,
                icon_data_uri: a.icon_data_uri,
            })
        })
        .collect())
}
pub fn get_catalog(refresh: bool) -> Result<Value, String> {
    if refresh {
        DATA_TARGETS.lock().map_err(|_| "libraryBusy")?.clear();
    }
    let cached = DISCOVERY.lock().map_err(|_| "libraryBusy")?.clone();
    let apps = match cached {
        Some((at, apps)) if !refresh && at.elapsed() < Duration::from_secs(180) => apps,
        _ => {
            let apps = discover()?;
            *DISCOVERY.lock().map_err(|_| "libraryBusy")? = Some((Instant::now(), apps.clone()));
            apps
        }
    };
    catalog(apps)
}
pub fn pick_folder() -> Result<Option<String>, String> {
    let out = crate::hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-STA",
            "-Command",
            include_str!("pick-folder.ps1"),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    let p = String::from_utf8(out.stdout).map_err(|e| e.to_string())?;
    if p.trim().is_empty() {
        Ok(None)
    } else {
        normalize(&p).map(Some)
    }
}
fn ensure_closed(a: &AppEntry) -> Result<(), String> {
    let mut c = crate::hidden_command("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ])
    .arg(workspace().join("crates/platform/src/app-in-use.ps1"))
    .args(["-Source", &a.path]);
    if let Some(j) = journal(&a.id) {
        if let Some(b) = j["backing"].as_str() {
            c.args(["-Backing", b]);
        }
    }
    let s = c.status().map_err(|e| e.to_string())?;
    if s.success() {
        Ok(())
    } else {
        Err(if s.code() == Some(2) {
            "libraryCloseApp"
        } else {
            "libraryProcessCheckFailed"
        }
        .into())
    }
}
fn row(a: &AppEntry, managed: bool, configs: &[CacheConfiguration]) -> Value {
    let p = phase(&a.id);
    let n = raw(&a.id);
    let c = configs.iter().find(|c| on_source(a, c));
    let files = if matches!(p.as_str(), "active" | "paused") {
        n.as_ref()
            .and_then(|n| n["cachedFiles"].as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    let shared = c.is_some();
    json!({"sharedPool":shared,"cachePolicy":"loadingPriority","sharedCapacityBytes":if shared{c.map(|c|c.budget_bytes)}else{None},"id":a.id,"name":a.name,"path":a.path,"origin":a.origin,"kind":a.kind,"iconDataUri":a.icon_data_uri,"managed":managed,"phase":p,"eligible":c.is_some(),"reason":if c.is_some(){None}else{Some("configChooseDrive")},"cacheUsedBytes":if p=="stopped"{0}else{n.as_ref().and_then(|n|n["occupiedBytes"].as_u64()).unwrap_or(0)},"allocationBytes":if shared{0}else{allocation(&a.id)},"canControl":matches!(p.as_str(),"active"|"paused"|"faulted"),"cachedFiles":files,"configurationId":c.map(|c|&c.id),"sourceLetter":c.map(|c|&c.source_letter),"ssdLetter":c.map(|c|&c.ssd_letter)})
}
fn data_target(a: &AppEntry) -> Option<String> {
    if a.kind != "game" {
        return None;
    }
    if let Ok(cache) = DATA_TARGETS.lock() {
        if let Some((at, path)) = cache.get(&a.path) {
            if at.elapsed() < Duration::from_secs(180) {
                return path.clone();
            }
        }
    }
    let path = data_scope::find(Path::new(&a.path)).map(|p| p.to_string_lossy().into_owned());
    if let Ok(mut cache) = DATA_TARGETS.lock() {
        if cache.len() >= 512 {
            cache.clear();
        }
        cache.insert(a.path.clone(), (Instant::now(), path.clone()));
    }
    path
}
fn data_child<'a>(s: &'a Store, a: &AppEntry, target: Option<&str>) -> Option<&'a AppEntry> {
    s.apps.iter().find(|b| {
        b.id != a.id
            && b.path
                .to_lowercase()
                .starts_with(&(a.path.to_lowercase() + "\\"))
            && (s.data_links.get(&a.id) == Some(&b.id)
                || target.is_some_and(|p| p.eq_ignore_ascii_case(&b.path)))
    })
}
fn selected_mode(
    s: &Store,
    a: &AppEntry,
    child: Option<&AppEntry>,
    target: Option<&str>,
) -> &'static str {
    if phase(&a.id) != "stopped" {
        return "full";
    }
    if child.is_some_and(|c| phase(&c.id) != "stopped") {
        return "dataOnly";
    }
    match s.cache_modes.get(&a.id).map(String::as_str) {
        Some("full") => "full",
        Some("dataOnly") => "dataOnly",
        _ if child.is_some() || target.is_some() => "dataOnly",
        _ => "full",
    }
}
pub fn catalog(discovered: Vec<AppEntry>) -> Result<Value, String> {
    let s = load()?;
    let mut items: BTreeMap<String, AppEntry> =
        discovered.into_iter().map(|a| (a.id.clone(), a)).collect();
    for a in &s.apps {
        let mut a = a.clone();
        if let Some(found) = items.get(&a.id) {
            if found.icon_data_uri.is_some() {
                a.icon_data_uri = found.icon_data_uri.clone()
            }
            if found.kind == "game" {
                a.kind = found.kind.clone()
            }
        }
        items.insert(a.id.clone(), a);
    }
    let targets: BTreeMap<_, _> = items
        .values()
        .map(|a| (a.id.clone(), data_target(a)))
        .collect();
    let links: BTreeMap<_, _> = items
        .values()
        .filter_map(|a| {
            data_child(&s, a, targets[&a.id].as_deref()).map(|c| (a.id.clone(), c.clone()))
        })
        .collect();
    let hidden: Vec<_> = links
        .iter()
        .filter(|(id, c)| phase(id) == "stopped" || phase(&c.id) == "stopped")
        .map(|(_, c)| c.id.as_str())
        .collect();
    let rows: Vec<_> = items
        .values()
        .filter(|a| !hidden.contains(&a.id.as_str()))
        .map(|a| {
            let child = links.get(&a.id);
            let target = child
                .filter(|c| phase(&c.id) != "stopped")
                .map(|c| c.path.as_str())
                .or(targets[&a.id].as_deref())
                .or_else(|| child.map(|c| c.path.as_str()));
            let mode = selected_mode(&s, a, child, target);
            let effective = if phase(&a.id) == "stopped" {
                child.filter(|c| phase(&c.id) != "stopped").unwrap_or(a)
            } else {
                a
            };
            let mut value = row(
                effective,
                s.apps.iter().any(|b| b.id == a.id) || child.is_some(),
                &s.configurations,
            );
            for (key, v) in [
                ("id", json!(a.id)),
                ("name", json!(a.name)),
                ("path", json!(a.path)),
                ("origin", json!(a.origin)),
                ("kind", json!(a.kind)),
                ("iconDataUri", json!(a.icon_data_uri)),
            ] {
                value[key] = v;
            }
            value["cacheMode"] = json!(mode);
            value["dataCachePath"] = json!(target);
            value["dataCacheAvailable"] = json!(target.is_some());
            value
        })
        .collect();
    let configs: Vec<_> = s
        .configurations
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap();
            v["allocatedBytes"] = json!(allocated(&active_for(&s, c)));
            v["activeCount"] = json!(active_for(&s, c).len());
            v["cacheUsedBytes"] = json!(resident(&active_for(&s, c)));
            v
        })
        .collect();
    Ok(
        json!({"cacheUsedBytes":resident(&active(&s)),"cachePolicy":"loadingPriority","apps":rows,"configurations":configs,"budgetBytes":s.configurations.iter().map(|c|c.budget_bytes).sum::<u64>(),"allocatedBytes":allocated(&s.apps),"available":available(),"legacyActive":legacy_active()}),
    )
}
pub fn state() -> Result<Value, String> {
    catalog(vec![])
}
pub fn add(name: Option<String>, path: String) -> Result<Value, String> {
    let _m = MUTATION.lock().map_err(|_| "libraryBusy")?;
    let _lock = lock()?;
    let mut s = load()?;
    let path = normalize(&path)?;
    if !Path::new(&path).is_dir() {
        return Err("libraryFolderMissing".into());
    }
    let id = app_id(&path);
    if !s.apps.iter().any(|a| a.id == id) {
        s.apps.push(AppEntry {
            id,
            name: name
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| path.rsplit('\\').next().unwrap_or(&path).into()),
            path,
            origin: "Manual".into(),
            kind: "app".into(),
            icon_data_uri: None,
        });
        save(&s)?
    }
    // Refresh discovery on the next UI read so a newly added manual folder gets
    // its local icon immediately rather than waiting for the three-minute TTL.
    *DISCOVERY.lock().map_err(|_| "libraryBusy")? = None;
    drop(_lock);
    state()
}
fn validate_devices(c: &CacheConfiguration, snapshot: &Snapshot) -> Result<(), String> {
    if !valid_letter(&c.source_letter)
        || !valid_letter(&c.ssd_letter)
        || c.source_disk_id == c.ssd_disk_id
    {
        return Err("configInvalidDevices".into());
    }
    let source = snapshot
        .disks
        .iter()
        .find(|d| d.id == c.source_disk_id)
        .ok_or("configDeviceMissing")?;
    let ssd = snapshot
        .disks
        .iter()
        .find(|d| d.id == c.ssd_disk_id)
        .ok_or("configDeviceMissing")?;
    let a = source
        .volumes
        .iter()
        .find(|v| v.letter == c.source_letter)
        .ok_or("configDeviceChanged")?;
    let b = ssd
        .volumes
        .iter()
        .find(|v| v.letter == c.ssd_letter)
        .ok_or("configDeviceChanged")?;
    if source.kind != "HDD"
        || ssd.kind != "SSD"
        || !source.online
        || !ssd.online
        || source.read_only
        || ssd.read_only
        || a.system
        || b.system
        || a.file_system != "NTFS"
        || b.file_system != "NTFS"
        || a.health != "Healthy"
        || b.health != "Healthy"
    {
        return Err("configInvalidDevices".into());
    }
    Ok(())
}
fn validate_identities(c: &CacheConfiguration) -> Result<(), String> {
    if c.source_volume_id.is_empty()
        || c.ssd_volume_id.is_empty()
        || !volume_identity(&c.source_letter)?.eq_ignore_ascii_case(&c.source_volume_id)
        || !volume_identity(&c.ssd_letter)?.eq_ignore_ascii_case(&c.ssd_volume_id)
    {
        return Err("configDeviceChanged".into());
    }
    Ok(())
}
pub fn benchmark_configuration(
    id: &str,
    snapshot: &Snapshot,
    fixture_bytes: u64,
) -> Result<Value, String> {
    let s = load()?;
    let c = s
        .configurations
        .iter()
        .find(|c| c.id == id)
        .ok_or("configNotFound")?;
    validate_devices(c, snapshot)?;
    validate_identities(c)?;
    for app in s.apps.iter().filter(|a| a.kind == "game") {
        ensure_closed(app)?;
    }
    let source = snapshot
        .disks
        .iter()
        .find(|d| d.id == c.source_disk_id)
        .ok_or("configDeviceMissing")?;
    let ssd = snapshot
        .disks
        .iter()
        .find(|d| d.id == c.ssd_disk_id)
        .ok_or("configDeviceMissing")?;
    let a = source
        .volumes
        .iter()
        .find(|v| v.letter == c.source_letter)
        .ok_or("configDeviceMissing")?;
    let b = ssd
        .volumes
        .iter()
        .find(|v| v.letter == c.ssd_letter)
        .ok_or("configDeviceMissing")?;
    if a.free < fixture_bytes + 1024 * 1024 * 1024
        || b.free < fixture_bytes + 192 * 1024 * 1024 + (b.size / 10).max(10 * 1024 * 1024 * 1024)
    {
        return Err("benchmarkSpace".into());
    }
    Ok(
        json!({"sourceLetter":c.source_letter,"ssdLetter":c.ssd_letter,"sourceVolumeId":c.source_volume_id,"ssdVolumeId":c.ssd_volume_id,"hardware":{"source":{"model":source.model,"bus":source.bus,"kind":source.kind,"sizeBytes":source.size,"letter":c.source_letter},"ssd":{"model":ssd.model,"bus":ssd.bus,"kind":ssd.kind,"sizeBytes":ssd.size,"letter":c.ssd_letter}}}),
    )
}
fn same_target(a: &CacheConfiguration, b: &CacheConfiguration) -> bool {
    a.ssd_disk_id == b.ssd_disk_id && a.ssd_letter == b.ssd_letter
}
fn capacity_check(s: &Store, c: &CacheConfiguration, snapshot: &Snapshot) -> Result<(), String> {
    let v = snapshot
        .disks
        .iter()
        .find(|d| d.id == c.ssd_disk_id)
        .and_then(|d| d.volumes.iter().find(|v| v.letter == c.ssd_letter))
        .ok_or("configDeviceMissing")?;
    let pool_count = s
        .configurations
        .iter()
        .filter(|b| b.id != c.id && same_target(b, c))
        .count()
        + 1;
    let committed = s
        .configurations
        .iter()
        .filter(|b| b.id != c.id && same_target(b, c))
        .map(|b| b.budget_bytes)
        .sum::<u64>()
        + c.budget_bytes
        + pool_count as u64 * 64 * 1024 * 1024;
    let owned = physical_allocated(
        &s.apps
            .iter()
            .filter(|a| {
                read(root().join(&a.id).join("request.json")).is_some_and(|r| {
                    r["ssdDiskId"] == c.ssd_disk_id && r["ssdLetter"] == c.ssd_letter
                })
            })
            .cloned()
            .collect::<Vec<_>>(),
    );
    let reserve = (v.size / 10).max(10 * MIN);
    if v.free < reserve.saturating_add(committed.saturating_sub(owned)) {
        return Err("configInsufficientSpace".into());
    }
    Ok(())
}
fn launch(action: &str, a: &AppEntry, capacity: u64) -> Result<(), String> {
    if action == "start" {
        recovery_helper(true)?;
    }
    fs::create_dir_all(dir(&a.id)?).map_err(|e| e.to_string())?;
    let mut c = crate::hidden_command("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ])
    .arg(workspace().join("tools/loading-cache/schedule-session.ps1"))
    .args(["-Action", action, "-AppId", &a.id]);
    if action == "start" {
        c.args(["-Source", &a.path, "-CapacityBytes", &capacity.to_string()]);
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir(&a.id)?.join("supervisor.log"))
        .map_err(|e| e.to_string())?;
    let s = c
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .status()
        .map_err(|e| e.to_string())?;
    if !s.success() {
        return Err("libraryHostFailed".into());
    }
    Ok(())
}
fn recovery_helper(register_only: bool) -> Result<(), String> {
    let mut command = crate::hidden_command("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(workspace().join("tools/loading-cache/recover-stale.ps1"))
        .arg("-RegisterLogon");
    if register_only { command.arg("-RegisterOnly"); }
    let output = command.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("Folder recovery initialization failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}
/// Restore orphaned library mounts at startup; live sessions are preserved.
pub fn initialize_recovery() -> Result<(), String> {
    let _guard = MUTATION.lock().map_err(|_| "Library lock poisoned")?;
    recovery_helper(false)
}
/// Present while a cache should be resumed by the logon task after a restart.
fn resume_marker(id: &str) -> Result<PathBuf, String> {
    Ok(dir(id)?.join("resume-at-logon"))
}
/// A user stop means the cache stays off after the next sign-in.
fn forget_resume(id: &str) {
    if let Ok(path) = resume_marker(id) {
        let _ = fs::remove_file(path);
    }
}
/// Discards a stopped profile's retained shared-pool records. Stopping keeps
/// them for the next start; a missing SSD or pool leaves nothing to discard.
fn purge(a: &AppEntry) {
    let Some(j) = journal(&a.id) else { return };
    if j["sharedPool"] != true || j["restored"] != true {
        return;
    }
    let Some(cache) = cache_file(&j, &a.id).filter(|p| p.exists()) else {
        return;
    };
    let capacity = j["capacityBytes"].as_u64().unwrap_or(0).to_string();
    let worker = workspace().join("artifacts/loading-cache/folder-worker.exe");
    let _ = crate::hidden_command(&worker.to_string_lossy())
        .arg("clear-owner")
        .arg(&cache)
        .arg(capacity)
        .arg(&a.id)
        .status();
}
fn resize(a: &AppEntry, capacity: u64) -> Result<(), String> {
    if allocation(&a.id) == capacity {
        return Ok(());
    }
    let j = journal(&a.id).ok_or("Missing app session")?;
    let t = task(&j, &a.id)?;
    atomic(&t.join("stop.resize"), capacity.to_string().as_bytes())?;
    let until = Instant::now() + Duration::from_secs(12);
    while Instant::now() < until {
        if raw(&a.id).is_some_and(|n| n["capacityBytes"] == capacity)
            && allocation(&a.id) == capacity
        {
            return Ok(());
        }
        if !matches!(phase(&a.id).as_str(), "active" | "paused") {
            return Err("libraryRecoveryRequired".into());
        }
        thread::sleep(Duration::from_millis(100))
    }
    Err("libraryResizeFailed".into())
}
fn control(a: &AppEntry, op: &str) -> Result<(), String> {
    let Some(j) = journal(&a.id) else {
        return if op == "stop" {
            Ok(())
        } else {
            Err("libraryNotActive".into())
        };
    };
    let t = task(&j, &a.id)?;
    if op == "stop"
        && !crate::folder_runtime::alive(&j, "worker")
        && !crate::folder_runtime::alive(&j, "supervisor")
    {
        launch("recover", a, 0)?;
    } else {
        let name = match op {
            "stop" => "stop",
            "pause" => "stop.pause",
            "resume" => "stop.resume",
            "clear" => "stop.clear",
            _ => return Err("Invalid control".into()),
        };
        atomic(&t.join(name), b"control")?;
    }
    let until = Instant::now() + Duration::from_secs(35);
    while Instant::now() < until {
        let n = raw(&a.id);
        let done = match op {
            "stop" => journal(&a.id).is_some_and(|j| j["restored"] == true),
            "pause" => n.is_some_and(|n| n["paused"] == true),
            "resume" => n.is_some_and(|n| n["paused"] == false),
            "clear" => !t.join("stop.clear").exists(),
            _ => false,
        };
        if done {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100))
    }
    Err("libraryOperationTimeout".into())
}
fn update_configuration(
    s: &mut Store,
    mut c: CacheConfiguration,
    snapshot: &Snapshot,
) -> Result<(), String> {
    c.source_letter = c.source_letter.to_ascii_uppercase();
    c.ssd_letter = c.ssd_letter.to_ascii_uppercase();
    c.budget_bytes = c.budget_bytes / BLOCK * BLOCK;
    if !(MIN..=MAX).contains(&c.budget_bytes) || c.name.len() > 160 {
        return Err("configInvalid".into());
    }
    validate_devices(&c, snapshot)?;
    let old = s.configurations.iter().find(|b| b.id == c.id).cloned();
    if old.is_none() {
        if !c.id.is_empty() {
            return Err("configNotFound".into());
        }
        if s.configurations.len() >= 8 {
            return Err("configTooMany".into());
        }
        if s.configurations
            .iter()
            .any(|b| b.source_letter == c.source_letter)
        {
            return Err("configSourceInUse".into());
        }
        c.id = configuration_id(&c);
    }
    if s.configurations
        .iter()
        .any(|b| b.id != c.id && b.source_letter == c.source_letter)
    {
        return Err("configSourceInUse".into());
    }
    let live = old.as_ref().map(|b| active_for(s, b)).unwrap_or_default();
    let pair_changed = old.as_ref().is_some_and(|b| {
        b.source_disk_id != c.source_disk_id
            || b.source_letter != c.source_letter
            || !same_target(b, &c)
    });
    if !live.is_empty() && pair_changed {
        return Err("configStopFirst".into());
    }
    if let Some(old) = &old {
        if !pair_changed && !live.is_empty() {
            validate_identities(old)?;
        }
    }
    c.source_volume_id = volume_identity(&c.source_letter)?;
    c.ssd_volume_id = volume_identity(&c.ssd_letter)?;
    if c.name.trim().is_empty() {
        c.name = format!("{} → {}", c.source_letter, c.ssd_letter)
    }
    capacity_check(s, &c, snapshot)?;
    let share = if live.is_empty() {
        None
    } else {
        if live
            .iter()
            .any(|a| !matches!(phase(&a.id).as_str(), "active" | "paused"))
        {
            return Err("libraryRecoveryRequired".into());
        }
        Some(c.budget_bytes)
    };
    // Persist an increased ceiling before grows; shrink fully before lowering it.
    if old
        .as_ref()
        .is_some_and(|b| c.budget_bytes > b.budget_bytes)
    {
        s.configurations.retain(|b| b.id != c.id);
        s.configurations.push(c.clone());
        save(s)?;
    }
    if let Some(capacity) = share {
        if let Some(a) = live.first() {
            resize(a, capacity)?;
        }
    }
    s.configurations.retain(|b| b.id != c.id);
    s.configurations.push(c);
    save(s)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigurationCommand {
    pub operation: String,
    pub id: Option<String>,
    pub configuration: Option<CacheConfiguration>,
}
pub fn configuration_operation(
    command: ConfigurationCommand,
    snapshot: Option<Snapshot>,
) -> Result<Value, String> {
    let _m = MUTATION.lock().map_err(|_| "libraryBusy")?;
    let _lock = lock()?;
    let mut s = load()?;
    match command.operation.as_str() {
        "save" => update_configuration(
            &mut s,
            command.configuration.ok_or("configInvalid")?,
            &snapshot.ok_or("Missing devices")?,
        )?,
        "stop" | "remove" => {
            let c = s
                .configurations
                .iter()
                .find(|c| Some(c.id.as_str()) == command.id.as_deref())
                .cloned()
                .ok_or("configNotFound")?;
            let live = active_for(&s, &c);
            for a in &live {
                if phase(&a.id) != "faulted" {
                    ensure_closed(a)?
                }
            }
            for a in &live {
                forget_resume(&a.id);
                control(a, "stop")?
            }
            let mut pools = BTreeMap::new();
            for a in &s.apps {
                if let Some(j) = journal(&a.id) {
                    if j["sharedPool"] == true && j["configurationId"] == c.id {
                        if let Some(path) = cache_file(&j, &a.id) {
                            pools.insert(path, ());
                        }
                    }
                }
            }
            for path in pools.keys() {
                for owned in [
                    path.clone(),
                    PathBuf::from(format!("{}.index", path.display())),
                ] {
                    if owned.exists() {
                        fs::remove_file(owned).map_err(|e| e.to_string())?;
                    }
                }
            }
            if command.operation == "remove" {
                s.configurations.retain(|b| b.id != c.id);
            }
            save(&s)?;
        }
        _ => return Err("configInvalid".into()),
    }
    drop(_lock);
    state()
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LibraryCommand {
    pub operation: String,
    pub id: Option<String>,
    pub app: Option<AppEntry>,
    pub budget_bytes: Option<u64>,
}
pub fn operate(command: LibraryCommand, snapshot: Option<Snapshot>) -> Result<Value, String> {
    let _m = MUTATION.lock().map_err(|_| "libraryBusy")?;
    let _lock = lock()?;
    let mut s = load()?;
    if command.operation == "budget" {
        if s.configurations.len() != 1 {
            return Err("configChooseBudget".into());
        }
        let mut c = s.configurations[0].clone();
        c.budget_bytes = command.budget_bytes.ok_or("Missing budget")?;
        update_configuration(&mut s, c, &crate::discover()?)?;
        drop(_lock);
        return state();
    }
    let id = command
        .id
        .as_deref()
        .or_else(|| command.app.as_ref().map(|a| a.id.as_str()))
        .ok_or("Missing app ID")?;
    if matches!(
        command.operation.as_str(),
        "start" | "modeData" | "modeFull"
    ) && !s.apps.iter().any(|a| a.id == id)
    {
        let a = command.app.clone().ok_or("libraryChooseApp")?;
        let path = normalize(&a.path)?;
        if app_id(&path) != a.id {
            return Err("Invalid app identity".into());
        }
        s.apps.push(AppEntry { path, ..a });
        save(&s)?;
    }
    let mut parent = s
        .apps
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("libraryChooseApp")?;
    if command.app.as_ref().is_some_and(|a| {
        a.id == parent.id && a.path.eq_ignore_ascii_case(&parent.path) && a.kind == "game"
    }) {
        parent.kind = "game".into();
        if let Some(saved) = s.apps.iter_mut().find(|a| a.id == parent.id) {
            saved.kind = "game".into();
        }
    }
    let target = data_target(&parent);
    let child = data_child(&s, &parent, target.as_deref()).cloned();
    if matches!(command.operation.as_str(), "modeData" | "modeFull") {
        if phase(&parent.id) != "stopped"
            || child.as_ref().is_some_and(|c| phase(&c.id) != "stopped")
        {
            return Err("libraryStopModeFirst".into());
        }
        if command.operation == "modeData" && target.is_none() {
            return Err("libraryDataUnsupported".into());
        }
        s.cache_modes.insert(
            parent.id.clone(),
            if command.operation == "modeData" {
                "dataOnly"
            } else {
                "full"
            }
            .into(),
        );
        if let Some(c) = &child {
            s.data_links.insert(parent.id.clone(), c.id.clone());
        }
        save(&s)?;
        drop(_lock);
        return state();
    }
    let mode = selected_mode(&s, &parent, child.as_ref(), target.as_deref());
    let a = if command.operation == "start" && mode == "dataOnly" {
        let path = target.as_ref().ok_or("libraryDataUnsupported")?;
        if child
            .as_ref()
            .is_some_and(|c| !c.path.eq_ignore_ascii_case(path))
        {
            return Err("libraryDataChanged".into());
        }
        if phase(&parent.id) != "stopped" {
            return Err("libraryAlreadyActive".into());
        }
        if child.as_ref().is_none_or(|c| phase(&c.id) == "stopped") {
            data_scope::validate(Path::new(&parent.path), Path::new(path))?;
        }
        ensure_closed(&parent)?;
        let data = child.clone().unwrap_or_else(|| AppEntry {
            id: app_id(path),
            name: format!("{} — Game data", parent.name),
            path: path.clone(),
            origin: "Manual".into(),
            kind: "app".into(),
            icon_data_uri: parent.icon_data_uri.clone(),
        });
        if !s.apps.iter().any(|b| b.id == data.id) {
            s.apps.push(data.clone());
        }
        s.data_links.insert(parent.id.clone(), data.id.clone());
        s.cache_modes.insert(parent.id.clone(), "dataOnly".into());
        data
    } else if command.operation != "start" && phase(&parent.id) == "stopped" {
        child
            .as_ref()
            .filter(|c| phase(&c.id) != "stopped")
            .cloned()
            .unwrap_or_else(|| parent.clone())
    } else {
        parent.clone()
    };
    match command.operation.as_str() {
        "start" => {
            if !Path::new(&a.path).is_dir() {
                return Err("libraryFolderMissing".into());
            }
            if !available() {
                return Err("folderRuntimeUnavailable".into());
            }
            if legacy_active() {
                return Err("libraryLegacyActive".into());
            }
            if phase(&a.id) != "stopped" {
                return Err("libraryAlreadyActive".into());
            }
            let all_live = active(&s);
            if all_live.len() >= 8 {
                return Err("libraryTooManyActive".into());
            }
            if all_live.iter().any(|b| overlaps(&a.path, &b.path)) {
                return Err("libraryOverlap".into());
            }
            let c = s
                .configurations
                .iter()
                .find(|c| on_source(&a, c))
                .cloned()
                .ok_or("configChooseDrive")?;
            let live = active_for(&s, &c);
            if live
                .iter()
                .any(|b| !matches!(phase(&b.id).as_str(), "active" | "paused"))
            {
                return Err("libraryRecoveryRequired".into());
            }
            let share = c.budget_bytes;
            let snapshot = snapshot.ok_or("Missing device snapshot")?;
            validate_devices(&c, &snapshot)?;
            validate_identities(&c)?;
            capacity_check(&s, &c, &snapshot)?;
            ensure_closed(&a)?;
            let request = CacheRequest {
                source_disk_id: c.source_disk_id.clone(),
                source_letter: c.source_letter.clone(),
                ssd_disk_id: c.ssd_disk_id.clone(),
                ssd_letter: c.ssd_letter.clone(),
                capacity_bytes: share,
                scope: Some(CacheScope::Selected {
                    folders: vec![a.path.clone()],
                }),
            };
            fs::create_dir_all(dir(&a.id)?).map_err(|e| e.to_string())?;
            atomic(
                &dir(&a.id)?.join("request.json"),
                &serde_json::to_vec(&request).map_err(|e| e.to_string())?,
            )?;
            atomic(&dir(&a.id)?.join("session-plan.json"),&serde_json::to_vec(&json!({"configurationId":c.id,"request":request,"sourceVolumeId":c.source_volume_id,"ssdVolumeId":c.ssd_volume_id})).map_err(|e|e.to_string())?)?;
            save(&s)?;
            let previous = journal(&a.id).and_then(|j| j["id"].as_str().map(str::to_owned));
            let _ = fs::remove_file(dir(&a.id)?.join("startup-error.json"));
            launch("start", &a, share)?;
            let until = Instant::now() + Duration::from_secs(40);
            while Instant::now() < until {
                if let Some(error) = read(dir(&a.id)?.join("startup-error.json"))
                    .and_then(|e| e["error"].as_str().map(str::to_owned))
                {
                    return Err(error);
                }
                if phase(&a.id) == "active" {
                    let _ = atomic(&resume_marker(&a.id)?, b"resume");
                    drop(_lock);
                    return state();
                }
                if let Some(j) = journal(&a.id) {
                    if j["id"].as_str() != previous.as_deref() {
                        if let Some(error) = j["error"].as_str() {
                            return Err(error.into());
                        }
                    }
                }
                thread::sleep(Duration::from_millis(100))
            }
            if let Some(j) = journal(&a.id) {
                if let Ok(t) = task(&j, &a.id) {
                    let _ = atomic(&t.join("stop"), b"stop");
                }
            }
            return Err("libraryOperationTimeout".into());
        }
        "clear" if phase(&a.id) == "stopped" => purge(&a),
        "pause" | "resume" | "clear" | "stop" => {
            if command.operation == "stop" && phase(&a.id) != "faulted" {
                ensure_closed(&a)?
            }
            if command.operation == "stop" {
                forget_resume(&a.id);
            }
            control(&a, &command.operation)?;
        }
        "remove" => {
            if phase(&parent.id) != "stopped"
                || child.as_ref().is_some_and(|c| phase(&c.id) != "stopped")
            {
                return Err("libraryStopFirst".into());
            }
            let mut removed = vec![parent.id.clone()];
            let mut entries = vec![parent.clone()];
            if let Some(c) = &child {
                entries.push(c.clone());
                removed.push(c.id.clone());
            }
            for entry in entries {
                if dir(&entry.id)?.exists() {
                    forget_resume(&entry.id);
                    purge(&entry);
                    launch("remove", &entry, 0)?;
                }
            }
            s.apps.retain(|b| !removed.contains(&b.id));
            for id in &removed {
                s.cache_modes.remove(id);
            }
            s.data_links
                .retain(|k, v| !removed.contains(k) && !removed.contains(v));
            if let Ok(mut cache) = DATA_TARGETS.lock() {
                cache.remove(&parent.path);
            }
            save(&s)?;
        }
        _ => return Err("Invalid library operation".into()),
    }
    drop(_lock);
    state()
}
pub fn aggregate() -> Option<Value> {
    let s = load().ok()?;
    let live = active(&s);
    if live.is_empty() {
        return None;
    }
    let mut files = vec![];
    let used = resident(&live);
    let capacity = allocated(&live);
    let fault = live.iter().any(|a| phase(&a.id) == "faulted");
    let starting = live.iter().any(|a| phase(&a.id) == "starting");
    let paused = live.iter().all(|a| phase(&a.id) == "paused");
    for a in &live {
        if let Some(n) = raw(&a.id) {
            if let Some(f) = n["cachedFiles"].as_array() {
                files.extend(f.iter().cloned())
            }
        }
    }
    Some(
        json!({"cachePolicy":"loadingPriority","backend":"dokanLibrary","phase":if fault{"faulted"}else if starting{"starting"}else if paused{"paused"}else{"active"},"canStart":false,"canControl":!starting,"canRemove":false,"reason":if fault{Some("libraryRecoveryRequired")}else{None},"cacheUsedBytes":used,"capacityBytes":capacity,"cachedFiles":files,"nativeVersion":null,"sourceVolume":if s.configurations.len()==1{Some(&s.configurations[0].source_letter)}else{None}}),
    )
}
pub fn control_all(op: &str) -> Result<Value, String> {
    let s = load()?;
    let items = active(&s);
    if op == "stop" {
        for a in &items {
            if phase(&a.id) != "faulted" {
                ensure_closed(a)?
            }
        }
    }
    for a in items {
        operate(
            LibraryCommand {
                operation: op.into(),
                id: Some(a.id),
                app: None,
                budget_bytes: None,
            },
            None,
        )?;
    }
    Ok(aggregate().unwrap_or_else(crate::folder_runtime::state))
}
pub fn telemetry() -> Option<crate::driver::Telemetry> {
    let s = load().ok()?;
    let live = active(&s);
    if live.is_empty()
        || live
            .iter()
            .any(|a| !matches!(phase(&a.id).as_str(), "active" | "paused"))
    {
        return None;
    }
    let mut t = crate::driver::Telemetry {
        protocol_version: 10,
        experimental_attach_enabled: false,
        attached: true,
        paused: live.iter().all(|a| phase(&a.id) == "paused"),
        hits: 0,
        misses: 0,
        source_read_bytes: 0,
        ssd_read_bytes: 0,
        fill_bytes: 0,
        evictions: 0,
        errors: 0,
        generation: 0,
        capacity_bytes: 0,
        source_length: 0,
        active_mutations: 0,
        pending_reads: 0,
        cache_disabled: false,
        read_bypasses: 0,
        queue_limit: 1,
        max_read_bytes: 65536,
        native_volume: None,
    };
    let mut seen_pools = std::collections::BTreeSet::new();
    for a in &live {
        let n = raw(&a.id)?;
        t.hits += n["hits"].as_u64()?;
        t.misses += n["misses"].as_u64()?;
        t.source_read_bytes += n["sourceReadBytes"].as_u64()?;
        t.ssd_read_bytes += n["ssdReadBytes"].as_u64()?;
        t.fill_bytes += n["fillBytes"].as_u64()?;
        let pool = journal(&a.id).and_then(|j| cache_file(&j, &a.id));
        if seen_pools.insert(pool) {
            t.evictions += n["evictions"].as_u64()?;
            t.capacity_bytes += n["capacityBytes"].as_u64()?;
        }
        t.errors += n["cacheErrors"].as_u64()? + n["checksumFailures"].as_u64()?;
        t.generation = t
            .generation
            .wrapping_add(journal(&a.id)?["supervisorStartTicks"].as_u64()?);
        t.read_bypasses += n["admissionBypasses"].as_u64().unwrap_or(0);
    }
    Some(t)
}
pub fn start_request(request: CacheRequest, snapshot: Snapshot) -> Result<Value, String> {
    let Some(CacheScope::Selected { folders }) = request.scope else {
        return Err("folderChooseOne".into());
    };
    if folders.len() != 1 {
        return Err("folderChooseOne".into());
    }
    let path = normalize(&folders[0])?;
    let s = load()?;
    let mut c = s
        .configurations
        .iter()
        .find(|c| {
            c.source_disk_id == request.source_disk_id
                && c.source_letter == request.source_letter
                && c.ssd_disk_id == request.ssd_disk_id
                && c.ssd_letter == request.ssd_letter
        })
        .cloned()
        .ok_or("configChooseDrive")?;
    c.budget_bytes = request.capacity_bytes;
    configuration_operation(
        ConfigurationCommand {
            operation: "save".into(),
            id: None,
            configuration: Some(c),
        },
        Some(snapshot.clone()),
    )?;
    let app = AppEntry {
        id: app_id(&path),
        name: path.rsplit('\\').next().unwrap_or(&path).into(),
        path,
        origin: "Manual".into(),
        kind: "app".into(),
        icon_data_uri: None,
    };
    operate(
        LibraryCommand {
            operation: "start".into(),
            id: Some(app.id.clone()),
            app: Some(app),
            budget_bytes: None,
        },
        Some(snapshot),
    )?;
    Ok(aggregate().unwrap_or_else(crate::folder_runtime::state))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_boundaries() {
        assert!(overlaps("D:\\Games\\A", "d:\\games\\a\\data"));
        assert!(!overlaps("D:\\Games\\A", "D:\\Games\\AB"));
        for p in [
            "D:\\Games\\..\\Other",
            "D:\\Games\\A:stream",
            "D:\\",
            "D:\\Games\\A\0file",
            "D:\\Games\\A\tfile",
        ] {
            assert!(normalize(p).is_err())
        }
        assert_eq!(normalize("e:\\a").unwrap(), "E:\\a");
        assert_eq!(app_id("D:\\Games\\A"), app_id("d:\\games\\a"));
    }
    #[test]
    fn shared_pool_path_is_bounded() {
        let id = "a".repeat(32);
        let config = "b".repeat(32);
        let session = "c".repeat(32);
        let mut j = json!({"id":session,"task":dir(&id).unwrap().join(&session),"sharedPool":true,"configurationId":config,"cacheDrive":"M:","cacheOwner":"0123456789ABCDEF","cache":format!("M:\\SuperHDD-Cache\\0123456789ABCDEF\\pools\\{}\\cache.bin",config)});
        assert!(cache_file(&j, &id).is_some());
        j["configurationId"] = json!("..");
        assert!(cache_file(&j, &id).is_none());
    }
    #[test]
    fn cache_path_is_bounded() {
        let id = "a".repeat(32);
        let n = "b".repeat(32);
        let owner = "A".repeat(16);
        let task = dir(&id).unwrap().join(&n);
        let mut j = json!({"id":n,"task":task,"cacheDrive":"F:","cacheOwner":owner,"cache":format!("F:\\SuperHDD-Cache\\{}\\{}\\{}\\cache.bin",owner,id,n)});
        assert!(cache_file(&j, &id).is_some());
        j["cache"] = json!("F:\\other.bin");
        assert!(cache_file(&j, &id).is_none());
        j["cacheDrive"] = json!("F:\\other");
        assert!(cache_file(&j, &id).is_none());
    }
}
