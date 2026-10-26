//! Local, bounded, read-only named-pipe service contract. No arbitrary commands,
//! paths, downloads, or privileged storage mutations are accepted by this endpoint.
use crate::{CacheRequest, DiscoveryCache, Snapshot};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    mem::size_of,
    os::windows::io::AsRawHandle,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{
        Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::*,
    System::{Pipes::*, Services::*},
};
const PIPE: &str = "\\\\.\\pipe\\SuperHDD.Control.v1";
const ADMIN_PIPE: &str = "\\\\.\\pipe\\SuperHDD.Admin.v5";
const NATIVE_PIPE: &str = "\\\\.\\pipe\\SuperHDD.Native.v6";
const NATIVE_LAB_PIPE: &str = "\\\\.\\pipe\\SuperHDD.NativeLab.v6";
const LIMIT: usize = 65536;
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn serve(stop: Arc<AtomicBool>, cache: &DiscoveryCache) -> Result<(), String> {
    serve_named(PIPE, stop, cache)
}
fn serve_named(name: &str, stop: Arc<AtomicBool>, cache: &DiscoveryCache) -> Result<(), String> {
    serve_handler(name, stop, cache, false, &mut |_| None)
}
pub fn serve_with_status(
    stop: Arc<AtomicBool>,
    cache: &DiscoveryCache,
    mut handler: impl FnMut(&Value) -> Option<Value>,
) -> Result<(), String> {
    serve_handler(PIPE, stop, cache, false, &mut handler)
}
pub fn serve_admin(
    stop: Arc<AtomicBool>,
    cache: &DiscoveryCache,
    mut handler: impl FnMut(&Value) -> Option<Value>,
) -> Result<(), String> {
    serve_handler(ADMIN_PIPE, stop, cache, true, &mut handler)
}
pub fn serve_native(
    lab: bool,
    stop: Arc<AtomicBool>,
    mut handler: impl FnMut(&Value) -> Option<Value>,
) -> Result<(), String> {
    serve_handler(
        if lab { NATIVE_LAB_PIPE } else { NATIVE_PIPE },
        stop,
        &DiscoveryCache::default(),
        false,
        &mut handler,
    )
}
fn serve_handler(
    name: &str,
    stop: Arc<AtomicBool>,
    cache: &DiscoveryCache,
    private: bool,
    handler: &mut dyn FnMut(&Value) -> Option<Value>,
) -> Result<(), String> {
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(if private {
                "D:P(A;;GA;;;SY)(A;;GA;;;BA)"
            } else {
                "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)"
            })
            .as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let raw = CreateNamedPipeW(
            wide(name).as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            LIMIT as u32,
            4096,
            1000,
            &security,
        );
        LocalFree(descriptor);
        if raw == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let pipe = Handle(raw);
        while !stop.load(Ordering::SeqCst) {
            let connected =
                ConnectNamedPipe(pipe.0, null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED;
            if !connected {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut buffer = [0u8; 4096];
            let mut read = 0u32;
            let mut oversized = false;
            while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
                let ok = ReadFile(
                    pipe.0,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut read,
                    null_mut(),
                );
                if ok == 0 && GetLastError() == ERROR_MORE_DATA {
                    oversized = true;
                    break;
                }
                if ok != 0 && read > 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if read > 0 {
                let reply = if oversized {
                    json!({"ok":false,"error":"Request exceeds 4096 bytes."})
                } else {
                    match serde_json::from_slice::<Value>(&buffer[..read as usize]) {
                        Ok(request)
                            if matches!(name, NATIVE_PIPE | NATIVE_LAB_PIPE)
                                && !matches!(
                                    request.get("command").and_then(Value::as_str),
                                    Some("cache_status" | "telemetry")
                                ) =>
                        {
                            json!({"ok":false,"error":"Native status endpoint is read-only and accepts status or telemetry only."})
                        }
                        Ok(request) => {
                            if let Some(reply) = if private
                                || matches!(
                                    request.get("command").and_then(Value::as_str),
                                    Some("cache_status" | "telemetry")
                                ) {
                                handler(&request)
                            } else {
                                None
                            } {
                                reply
                            } else {
                                match request.get("command").and_then(Value::as_str) {
                                    Some("telemetry") => match crate::driver::telemetry() {
                                        Ok(stats) => json!({"ok":true,"result":stats}),
                                        Err(error) => json!({"ok":false,"error":error}),
                                    },
                                    Some("snapshot") => match cache.get(
                                        request
                                            .get("force")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false),
                                    ) {
                                        Ok(snapshot) => json!({"ok":true,"result":snapshot}),
                                        Err(error) => json!({"ok":false,"error":error}),
                                    },
                                    Some("plan_cache") => {
                                        match serde_json::from_value::<CacheRequest>(
                                            request.get("request").cloned().unwrap_or(Value::Null),
                                        )
                                        .and_then(|r| {
                                            cache
                                                .get(true)
                                                .and_then(|s| crate::plan_cache(&s, r))
                                                .map_err(serde::de::Error::custom)
                                        }) {
                                            Ok(plan) => json!({"ok":true,"result":plan}),
                                            Err(error) => {
                                                json!({"ok":false,"error":error.to_string()})
                                            }
                                        }
                                    }
                                    _ => {
                                        json!({"ok":false,"error":"Unsupported operation. This pipe exposes read-only discovery and planning only."})
                                    }
                                }
                            }
                        }
                        Err(_) => json!({"ok":false,"error":"Invalid JSON request."}),
                    }
                };
                let bytes = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
                if bytes.len() + 4 <= LIMIT {
                    let mut framed = Vec::with_capacity(bytes.len() + 4);
                    framed.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    framed.extend_from_slice(&bytes);
                    let mut written = 0;
                    if WriteFile(
                        pipe.0,
                        framed.as_ptr(),
                        framed.len() as u32,
                        &mut written,
                        null_mut(),
                    ) != 0
                        && written as usize == framed.len()
                    {
                        // An application ACK bounds drain time without a blocking FlushFileBuffers.
                        let deadline = Instant::now() + Duration::from_secs(3);
                        let mut ack = [0u8; 8];
                        while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
                            let mut n = 0;
                            if ReadFile(
                                pipe.0,
                                ack.as_mut_ptr(),
                                ack.len() as u32,
                                &mut n,
                                null_mut(),
                            ) != 0
                                && n > 0
                            {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
            }
            DisconnectNamedPipe(pipe.0);
        }
        Ok(())
    }
}

/// Authenticate the peer against the SCM's process ID before trusting its data.
/// An unprivileged process cannot substitute a similarly named pipe endpoint.
fn request_read_only(request: Value) -> Result<Value, String> {
    request_authenticated(PIPE, request)
}
fn request_authenticated(name: &str, request: Value) -> Result<Value, String> {
    request_service(name, "SuperHDDService", request)
}
fn request_service(name: &str, owner: &str, request: Value) -> Result<Value, String> {
    let mut file = open_pipe(name, Duration::from_secs(1)).map_err(|e| e.to_string())?;
    unsafe {
        let mut server_pid = 0;
        if GetNamedPipeServerProcessId(file.as_raw_handle(), &mut server_pid) == 0 {
            return Err("Cannot identify service pipe peer.".into());
        }
        let manager = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return Err("Cannot query service manager.".into());
        }
        let service = OpenServiceW(manager, wide(owner).as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            CloseServiceHandle(manager);
            return Err("SuperHDDService is not installed.".into());
        }
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            service,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        ) != 0;
        CloseServiceHandle(service);
        CloseServiceHandle(manager);
        if !ok || status.dwProcessId != server_pid || status.dwCurrentState != SERVICE_RUNNING {
            return Err("Named-pipe server does not match the running Windows service.".into());
        }
    }
    file.write_all(&serde_json::to_vec(&request).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut length = [0u8; 4];
    file.read_exact(&mut length).map_err(|e| e.to_string())?;
    let length = u32::from_le_bytes(length) as usize;
    if length > LIMIT - 4 {
        return Err("Oversized service response.".into());
    }
    let mut bytes = vec![0u8; length];
    file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let _ = file.write_all(b"ACK");
    let response: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Service request failed.")
            .into());
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn open_pipe(name: &str, timeout: Duration) -> std::io::Result<std::fs::File> {
    let deadline = Instant::now() + timeout;
    loop {
        match OpenOptions::new().read(true).write(true).open(name) {
            Ok(file) => return Ok(file),
            // A reply's ACK can still be waiting for the single-instance server
            // to disconnect. Only retry this transient contention, with a bound;
            // absence/access errors and peer authentication remain unchanged.
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn snapshot(force: bool) -> Result<Snapshot, String> {
    serde_json::from_value(request_read_only(
        json!({"command":"snapshot","force":force}),
    )?)
    .map_err(|e| e.to_string())
}
pub fn telemetry() -> Result<crate::driver::Telemetry, String> {
    serde_json::from_value(request_read_only(json!({"command":"telemetry"}))?)
        .map_err(|e| e.to_string())
}
pub fn cache_status() -> Result<Value, String> {
    request_read_only(json!({"command":"cache_status"}))
}
pub fn native_status(lab: bool) -> Result<Value, String> {
    request_service(
        if lab { NATIVE_LAB_PIPE } else { NATIVE_PIPE },
        if lab {
            crate::native_package::LAB_SERVICE
        } else {
            crate::native_package::SERVICE
        },
        json!({"command":"cache_status"}),
    )
}
pub fn native_telemetry() -> Result<crate::driver::Telemetry, String> {
    serde_json::from_value(request_service(
        NATIVE_PIPE,
        crate::native_package::SERVICE,
        json!({"command":"telemetry"}),
    )?)
    .map_err(|e| e.to_string())
}
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheAction {
    Pause,
    Resume,
    Clear,
}
pub fn cache_action(action: CacheAction) -> Result<Value, String> {
    request_authenticated(
        ADMIN_PIPE,
        json!({"command":"cache_action","action":action}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_server(
        suffix: &str,
    ) -> (
        String,
        Arc<AtomicBool>,
        std::thread::JoinHandle<Result<(), String>>,
    ) {
        let name = format!(
            "\\\\.\\pipe\\SuperHDD.Test.{}.{}",
            std::process::id(),
            suffix
        );
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let server_name = name.clone();
        let thread =
            std::thread::spawn(move || serve_named(&server_name, flag, &DiscoveryCache::default()));
        (name, stop, thread)
    }
    fn open_test_pipe(name: &str) -> std::fs::File {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match OpenOptions::new().read(true).write(true).open(name) {
                Ok(file) => return file,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("{error}"),
            }
        }
    }
    #[test]
    fn busy_service_pipe_has_bounded_retry_and_accepts_request_bursts() {
        let (name, stop, thread) = test_server("busy-bursts");
        let mut file = open_test_pipe(&name);
        let started = Instant::now();
        let error = open_pipe(&name, Duration::from_millis(50)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_PIPE_BUSY as i32));
        assert!(started.elapsed() < Duration::from_secs(1));
        for index in 0..32 {
            if index > 0 {
                file = open_pipe(&name, Duration::from_secs(1)).unwrap();
            }
            file.write_all(br#"{"command":"unsupported"}"#).unwrap();
            let mut length = [0; 4];
            file.read_exact(&mut length).unwrap();
            let mut bytes = vec![0; u32::from_le_bytes(length) as usize];
            file.read_exact(&mut bytes).unwrap();
            let reply: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(reply["ok"], false);
            assert!(reply["error"]
                .as_str()
                .unwrap()
                .contains("Unsupported operation"));
            file.write_all(b"ACK").unwrap();
        }
        drop(file);
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap().unwrap();
    }
    #[test]
    fn malformed_and_oversized_pipe_requests_are_rejected() {
        for (suffix, request, expected) in [
            ("malformed", b"{".to_vec(), "Invalid JSON"),
            ("oversized", vec![b' '; 8192], "4096"),
        ] {
            let (name, stop, thread) = test_server(suffix);
            let mut file = open_test_pipe(&name);
            file.write_all(&request).unwrap();
            let mut length = [0; 4];
            file.read_exact(&mut length).unwrap();
            let mut bytes = vec![0; u32::from_le_bytes(length) as usize];
            file.read_exact(&mut bytes).unwrap();
            let reply: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(reply["ok"], false);
            assert!(reply["error"].as_str().unwrap().contains(expected));
            let _ = file.write_all(b"ACK");
            drop(file);
            stop.store(true, Ordering::SeqCst);
            thread.join().unwrap().unwrap();
        }
    }
    #[test]
    fn stalled_pipe_client_does_not_prevent_service_shutdown() {
        let (name, stop, thread) = test_server("stalled");
        let file = open_test_pipe(&name);
        std::thread::sleep(Duration::from_millis(60));
        let started = Instant::now();
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(file);
    }
    #[test]
    fn local_pipe_rejects_privileged_mutation_commands() {
        let name = format!("\\\\.\\pipe\\SuperHDD.Test.{}", std::process::id());
        let server_name = name.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread =
            std::thread::spawn(move || serve_named(&server_name, flag, &DiscoveryCache::default()));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut file = loop {
            match OpenOptions::new().read(true).write(true).open(&name) {
                Ok(file) => break file,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("{error}"),
            }
        };
        file.write_all(br#"{"command":"create_cache","path":"C:\\Windows"}"#)
            .unwrap();
        let mut length = [0u8; 4];
        file.read_exact(&mut length).unwrap();
        let mut bytes = vec![0; u32::from_le_bytes(length) as usize];
        file.read_exact(&mut bytes).unwrap();
        let reply: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"]
            .as_str()
            .unwrap()
            .contains("Unsupported operation"));
        file.write_all(b"ACK").unwrap();
        drop(file);
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap().unwrap();
    }
}
