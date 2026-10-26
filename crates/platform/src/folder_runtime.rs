//! Local selected-folder backend using the installed signed Dokan runtime.
//! The supervisor owns migration/recovery; the UI never renames folders itself.
use crate::{
    cache_runtime::{Command, Operation},
    CachePlan, CacheRequest, CacheScope, Snapshot,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

fn workspace() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_default();
    PathBuf::from(path.to_string_lossy().trim_start_matches("\\\\?\\"))
}
fn root() -> PathBuf {
    workspace().join("artifacts/folder-runtime")
}
fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
fn hash(path: &Path) -> Option<String> {
    Some(format!("{:X}", Sha256::digest(fs::read(path).ok()?)))
}
pub fn available() -> bool {
    let w = workspace();
    let Some(q) = read(&w.join("artifacts/folder-cache/qualified.json")) else {
        return false;
    };
    q["verified"] == true
        && hash(&w.join("artifacts/folder-cache/folder-worker.exe")).as_deref()
            == q["workerSha256"].as_str()
        && hash(&w.join("tools/folder-cache/session.ps1")).as_deref() == q["sessionSha256"].as_str()
        && hash(&w.join("tools/folder-cache/schedule-session.ps1")).as_deref()
            == q["launcherSha256"].as_str()
}
fn journal() -> Option<Value> {
    read(&root().join("current.json"))
}
fn task(j: &Value) -> Result<PathBuf, String> {
    let id = j["id"].as_str().ok_or("Invalid folder session")?;
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid folder session ID".into());
    }
    let expected = root().join(id);
    if Path::new(j["task"].as_str().ok_or("Missing task")?) != expected {
        return Err("Folder session path mismatch".into());
    }
    Ok(expected)
}
pub(crate) fn alive(j: &Value, prefix: &str) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    let Some(pid) = j[format!("{prefix}Pid")]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
    else {
        return false;
    };
    let Some(expected) = j[format!("{prefix}StartTicks")].as_u64() else {
        return false;
    };
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return false;
        }
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        let mut code = 0;
        let ok = GetProcessTimes(
            p,
            &mut times[0],
            &mut times[1],
            &mut times[2],
            &mut times[3],
        ) != 0
            && GetExitCodeProcess(p, &mut code) != 0;
        CloseHandle(p);
        let created =
            (u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime);
        ok && code == 259 && created.saturating_add(504_911_232_000_000_000) == expected
    }
}
pub fn raw() -> Option<Value> {
    let j = journal()?;
    read(&task(&j).ok()?.join("status.json"))
}
pub fn state() -> Value {
    let j = journal();
    let native = raw();
    let mut phase = "stopped";
    let mut reason = None;
    if let Some(j) = &j {
        if j["phase"] == "active"
            && alive(j, "supervisor")
            && alive(j, "worker")
            && native.as_ref().is_some_and(|n| n["active"] == true)
        {
            phase = if native.as_ref().is_some_and(|n| n["paused"] == true) {
                "paused"
            } else {
                "active"
            };
        } else if j["restored"] != true {
            phase = if alive(j, "supervisor") {
                "starting"
            } else {
                "faulted"
            };
            reason = Some(if phase == "faulted" {
                "folderRecoveryRequired"
            } else {
                "folderStarting"
            });
        }
    }
    let ready = available();
    if !ready && reason.is_none() {
        reason = Some("folderRuntimeUnavailable")
    }
    json!({"backend":"dokanFolder","phase":phase,"canStart":ready&&phase=="stopped","canControl":matches!(phase,"active"|"paused"|"faulted"),"canRemove":phase=="stopped"&&j.is_some(),"reason":reason,"nativeVersion":null,"sourceVolume":"D:","request":read(&root().join("request.json")),"cacheUsedBytes":if matches!(phase,"active"|"paused"){native.as_ref().and_then(|n|n["occupiedBytes"].as_u64())}else{Some(0)},"cachedFiles":if matches!(phase,"active"|"paused"){native.as_ref().map(|n|n["cachedFiles"].clone()).unwrap_or(json!([]))}else{json!([])}})
}
fn selected(request: &CacheRequest) -> Result<&str, String> {
    let Some(CacheScope::Selected { folders }) = &request.scope else {
        return Err("Choose a folder".into());
    };
    if folders.len() != 1 {
        return Err("folderChooseOne".into());
    }
    if request.source_letter != "D:" || request.ssd_letter != "M:" {
        return Err("folderCurrentDevicesOnly".into());
    }
    let source = &folders[0];
    if source.len() > 2048
        || !source.to_ascii_uppercase().starts_with("D:\\")
        || source.contains(['\r', '\n', '"'])
        || source.split('\\').any(|p| p == "." || p == "..")
        || source.split('\\').count() < 3
    {
        return Err("Invalid selected folder".into());
    }
    Ok(source)
}
pub fn plan(snapshot: &Snapshot, request: CacheRequest) -> Result<CachePlan, String> {
    let source = selected(&request)?;
    if !Path::new(source).is_dir() {
        return Err("Selected folder does not exist".into());
    }
    let mut plan = crate::plan_cache(snapshot, request)?;
    // This backend uses ordinary file handles above BitLocker; it never caches
    // raw encrypted sectors. Locked volumes fail the directory/open checks.
    // EFS files and reparse points are rejected by the supervisor and worker.
    // Keep the raw-volume planner's encryption rule unchanged for other backends.
    plan.reasons.retain(|r| {
        !matches!(
            r.as_str(),
            "selectedScopeUnavailable"
                | "driverNotLoaded"
                | "runtimeReleasePending"
                | "encryptionUnknown"
        )
    });
    if !available() {
        plan.reasons.push("folderRuntimeUnavailable".into())
    }
    plan.activation_available = plan.reasons.is_empty();
    Ok(plan)
}
fn supervisor(
    action: &str,
    source: Option<&str>,
    capacity: u64,
) -> Result<std::process::Child, String> {
    let mut cmd = crate::hidden_command("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ])
    .arg(workspace().join("tools/folder-cache/schedule-session.ps1"))
    .args(["-Action", action]);
    if let Some(source) = source {
        cmd.args(["-Source", source, "-CapacityBytes", &capacity.to_string()]);
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root().join("supervisor.log"))
        .map_err(|e| e.to_string())?;
    cmd.stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| e.to_string())
}
pub fn execute(command: Command, snapshot: Option<Snapshot>) -> Result<Value, String> {
    if matches!(command.operation, Operation::Start) != command.request.is_some() {
        return Err("Invalid folder operation".into());
    }
    if matches!(command.operation, Operation::Start) {
        if !available() {
            return Err("folderRuntimeUnavailable".into());
        }
        if state()["phase"] != "stopped" {
            return Err("Folder session already exists; stop or recover it first".into());
        }
        let request = command.request.ok_or("Missing folder request")?;
        let checked = plan(&snapshot.ok_or("Missing disk snapshot")?, request)?;
        if !checked.activation_available {
            return Err(checked.reasons.join(", "));
        }
        fs::create_dir_all(root()).map_err(|e| e.to_string())?;
        fs::write(
            root().join("request.json"),
            serde_json::to_vec(&checked.request).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let source = selected(&checked.request)?.to_owned();
        let mut child = supervisor("start", Some(&source), checked.request.capacity_bytes)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let state = state();
            if state["phase"] == "active" {
                return Ok(state);
            }
            if child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some_and(|s| !s.success())
            {
                return Err(journal()
                    .and_then(|j| j["error"].as_str().map(str::to_owned))
                    .unwrap_or("Folder supervisor did not start".into()));
            }
            thread::sleep(Duration::from_millis(100));
        }
        if let Some(j) = journal() {
            if let Ok(t) = task(&j) {
                let _ = fs::write(t.join("stop"), b"stop");
            }
        }
        return Err("Folder start timed out; restoration requested".into());
    }
    let j = journal().ok_or("No folder session")?;
    let task = task(&j)?;
    let filename = match command.operation {
        Operation::Pause => "stop.pause",
        Operation::Resume => "stop.resume",
        Operation::Clear => "stop.clear",
        Operation::Stop => "stop",
        Operation::Remove => {
            if j["restored"] != true || alive(&j, "worker") || alive(&j, "supervisor") {
                return Err("Stop and restore the folder first".into());
            }
            fs::remove_file(root().join("current.json")).map_err(|e| e.to_string())?;
            return Ok(state());
        }
        Operation::Start => unreachable!(),
    };
    fs::write(task.join(filename), b"control").map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(35);
    while Instant::now() < deadline {
        if matches!(command.operation, Operation::Stop) {
            let current = journal().ok_or("Missing folder journal")?;
            if current["restored"] == true {
                return Ok(state());
            }
            if !alive(&current, "supervisor") && !alive(&current, "worker") {
                let mut child = supervisor("recover", None, 0)?;
                let result = child.wait().map_err(|e| e.to_string())?;
                if !result.success() {
                    return Err("folderRecoveryRequired".into());
                }
                // The scheduled host restores asynchronously after registration.
                while Instant::now() < deadline {
                    if journal().is_some_and(|j| j["restored"] == true) {
                        return Ok(state());
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                return Err("folderRecoveryRequired".into());
            }
        } else if let Some(n) = raw() {
            let done = match command.operation {
                Operation::Pause => n["paused"] == true,
                Operation::Resume => n["paused"] == false,
                Operation::Clear => n["occupiedBytes"] == 0,
                _ => false,
            };
            if done {
                return Ok(state());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("Folder operation timed out; source backing preserved".into())
}
pub fn telemetry() -> Option<crate::driver::Telemetry> {
    if !matches!(state()["phase"].as_str(), Some("active" | "paused")) {
        return None;
    }
    let n = raw()?;
    let value = |k: &str| n[k].as_u64().unwrap_or(0);
    Some(crate::driver::Telemetry {
        protocol_version: 9,
        experimental_attach_enabled: false,
        attached: true,
        paused: n["paused"] == true,
        hits: value("hits"),
        misses: value("misses"),
        source_read_bytes: value("sourceReadBytes"),
        ssd_read_bytes: value("ssdReadBytes"),
        fill_bytes: value("fillBytes"),
        evictions: value("evictions"),
        errors: value("cacheErrors") + value("checksumFailures"),
        generation: journal()?["supervisorStartTicks"].as_u64()?,
        capacity_bytes: value("capacityBytes"),
        source_length: 0,
        active_mutations: 0,
        pending_reads: 0,
        cache_disabled: false,
        read_bypasses: value("sequentialBypasses"),
        queue_limit: 1,
        max_read_bytes: 65536,
        native_volume: None,
    })
}
