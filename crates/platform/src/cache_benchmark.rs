//! Owned-fixture benchmark jobs. No arbitrary paths, shell text or game-file writes.
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
static CONTROL: Mutex<()> = Mutex::new(());
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub action: String,
    pub configuration_id: Option<String>,
    pub profile: Option<String>,
    pub workload: Option<String>,
}
fn root() -> PathBuf {
    super::app_library::workspace().join("artifacts/cache-benchmark")
}
fn read(path: impl AsRef<Path>) -> Option<Value> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit())
}
fn current() -> Option<(String, PathBuf)> {
    let id = read(root().join("current.json"))?["runId"]
        .as_str()?
        .to_owned();
    if !valid_id(&id) {
        return None;
    }
    let path = root().join("runs").join(&id);
    Some((id, path))
}
fn write(path: impl AsRef<Path>, value: &Value) -> Result<(), String> {
    fs::write(path, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn process_alive(pid: u32, created: u64) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{
            GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | 0x00100000, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut a = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut b = a;
        let mut c = a;
        let mut d = a;
        let ok = GetProcessTimes(h, &mut a, &mut b, &mut c, &mut d) != 0
            && ((u64::from(a.dwHighDateTime) << 32) | u64::from(a.dwLowDateTime)) == created
            && WaitForSingleObject(h, 0) == 258;
        CloseHandle(h);
        ok
    }
}
pub fn status() -> Value {
    let Some((id, path)) = current() else {
        return json!({"phase":"idle","running":false,"canRecover":false,"report":null});
    };
    let mut state =
        read(path.join("state.json")).unwrap_or(json!({"runId":id,"phase":"starting","percent":0}));
    let runner = read(path.join("runner.json"));
    let alive = runner.as_ref().is_some_and(|r| {
        process_alive(
            r["pid"].as_u64().unwrap_or(0) as u32,
            r["createdFileTime"].as_u64().unwrap_or(0),
        )
    });
    let busy = matches!(
        state["phase"].as_str(),
        Some(
            "starting"
                | "preflight"
                | "creatingFixture"
                | "firstPass"
                | "warming"
                | "measuring"
                | "cleanup"
        )
    );
    if busy && !alive {
        state["phase"] = json!("interrupted");
        state["message"] =
            json!("Benchmark controller exited; recover its owned fixture before starting again.");
    }
    let report = read(path.join("share/report.json"));
    let needs_cleanup =
        read(path.join("internal-result.json")).is_none_or(|r| r["cleanupComplete"] != true);
    state["running"] = json!(alive);
    state["canRecover"] = json!(!alive && needs_cleanup && state["phase"] != "recovered");
    state["report"] = json!(report);
    state["exportReady"] = json!(!alive && path.join("export").is_dir() && report.is_some());
    state
}
fn spawn_job(id: &str, path: &Path, recover: bool) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.join("runner.log"))
        .map_err(|e| e.to_string())?;
    let mut command = crate::hidden_command("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(super::app_library::workspace().join("scripts/run-cache-benchmark.ps1"))
        .args(["-RunId", id]);
    if recover {
        command.arg("-RecoverOnly");
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut a = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut b = a;
    let mut c = a;
    let mut d = a;
    if unsafe { GetProcessTimes(child.as_raw_handle().cast(), &mut a, &mut b, &mut c, &mut d) } == 0
    {
        return Err("benchmarkProcessIdentity".into());
    }
    let created = (u64::from(a.dwHighDateTime) << 32) | u64::from(a.dwLowDateTime);
    write(
        path.join("runner.json"),
        &json!({"pid":child.id(),"createdFileTime":created}),
    )?;
    // Child continues if the desktop closes; the worker watches this runner.
    drop(child);
    Ok(())
}
pub fn execute(command: Command, snapshot: Option<crate::Snapshot>) -> Result<Value, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let _guard = CONTROL.lock().map_err(|_| "benchmarkBusy")?;
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    let _file_guard = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(root().join("control.lock"))
        .map_err(|_| "benchmarkBusy")?;
    let state = status();
    match command.action.as_str() {
        "start" => {
            if state["running"] == true {
                return Err("benchmarkBusy".into());
            }
            if state["canRecover"] == true {
                return Err("benchmarkRecoverFirst".into());
            }
            let profile = command.profile.as_deref().unwrap_or("standard");
            let workload = command.workload.as_deref().unwrap_or("mixed");
            if !matches!(workload, "mixed" | "small" | "large") {
                return Err("benchmarkProfile".into());
            }
            let bytes = match profile {
                "quick" => 64 * 1048576,
                "standard" => 256 * 1048576,
                _ => return Err("benchmarkProfile".into()),
            };
            let mut request = super::app_library::benchmark_configuration(
                command
                    .configuration_id
                    .as_deref()
                    .ok_or("configNotFound")?,
                &snapshot.ok_or("Missing devices")?,
                bytes,
            )?;
            if !root().join("bin/manifest.json").is_file() {
                return Err("benchmarkNotBuilt".into());
            }
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let id = format!(
                "{:x}",
                Sha256::digest(format!("{}-{nonce}", std::process::id()).as_bytes())
            )[..32]
                .to_owned();
            let path = root().join("runs").join(&id);
            fs::create_dir_all(root().join("runs")).map_err(|e| e.to_string())?;
            fs::create_dir(&path).map_err(|e| e.to_string())?;
            request["profile"] = json!(profile);
            request["workload"] = json!(workload);
            write(path.join("request.json"), &request)?;
            write(
                path.join("state.json"),
                &json!({"runId":id,"phase":"starting","percent":0}),
            )?;
            spawn_job(&id, &path, false)?;
            write(root().join("current.json"), &json!({"runId":id}))?;
        }
        "cancel" => {
            if state["running"] == true {
                let (_, path) = current().ok_or("benchmarkMissing")?;
                fs::write(path.join("cancel"), b"cancel").map_err(|e| e.to_string())?;
            }
        }
        "recover" => {
            if state["running"] == true {
                return Err("benchmarkBusy".into());
            }
            let (id, path) = current().ok_or("benchmarkMissing")?;
            write(
                path.join("state.json"),
                &json!({"runId":id,"phase":"cleanup","percent":90}),
            )?;
            spawn_job(&id, &path, true)?;
        }
        "openReport" | "openFolder" => {
            if state["running"] == true {
                return Err("benchmarkBusy".into());
            }
            let (_, path) = current().ok_or("benchmarkMissing")?;
            let target = if command.action == "openReport" {
                path.join("share/report.html")
            } else {
                path.join("export")
            };
            if !target.exists() {
                return Err("benchmarkMissing".into());
            }
            crate::hidden_command("explorer.exe")
                .arg(target)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("benchmarkAction".into()),
    }
    Ok(status())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_and_commands_are_bounded() {
        assert!(!valid_id("../other"));
        assert!(valid_id(&"a".repeat(32)));
        assert!(
            serde_json::from_value::<Command>(json!({"action":"start","source":"C:\\"})).is_err()
        );
    }
}
