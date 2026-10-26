//! Worker ABI. Source selection is exclusively a protected, journaled service
//! configuration; no arbitrary path/device/capacity is accepted through the ABI.
#![cfg(windows)]
use std::io::Read;
use std::{
    fs, io,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use superhdd_cache_core::FileCache;
use superhdd_storage_control::{
    loopback::{Opened, Pins},
    windows::protected::ProtectedDir,
};
struct Runtime {
    cache: FileCache,
    pins: Pins,
    run_root: ProtectedDir,
    source_bytes: u64,
    paused: bool,
    generation: u64,
}
static RUNTIME: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();
fn state() -> &'static Mutex<Option<Runtime>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}
fn invalid() -> io::Error {
    io::Error::other("Invalid protected native runtime request")
}
fn ffi(action: impl FnOnce() -> io::Result<()>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            eprintln!("native cache: {error}");
            -1
        }
        Err(_) => -2,
    }
}
fn with_runtime(action: impl FnOnce(&mut Runtime) -> io::Result<()>) -> io::Result<()> {
    let mut state = state().lock().map_err(|_| invalid())?;
    action(state.as_mut().ok_or_else(invalid)?)
}
fn bounds(source_bytes: u64, offset: u64, length: usize) -> io::Result<()> {
    if length == 0
        || length > 1024 * 1024
        || !length.is_multiple_of(512)
        || !offset.is_multiple_of(512)
        || offset
            .checked_add(length as u64)
            .is_none_or(|end| end > source_bytes)
    {
        return Err(invalid());
    }
    Ok(())
}
/// # Safety
/// `run` points to exactly `length` readable bytes for this synchronous call.
#[no_mangle]
pub unsafe extern "C" fn cache_runtime_open(run: *const u8, length: u32) -> i32 {
    ffi(|| {
        if run.is_null() || length != 32 {
            return Err(invalid());
        }
        let run = std::str::from_utf8(unsafe { std::slice::from_raw_parts(run, length as usize) })
            .map_err(|_| invalid())?;
        let mut state = state().lock().map_err(|_| invalid())?;
        if state.is_some() {
            return Err(invalid());
        }
        let opened = Opened::open(run).map_err(io::Error::other)?;
        let run_root =
            ProtectedDir::open(&opened.pins.root.path().join(run)).map_err(io::Error::other)?;
        let source_bytes = opened.pins.config.source.bytes;
        let mut cache = FileCache::from_owned_device_handles(
            opened.source,
            source_bytes,
            opened.payload,
            opened.pins.config.capacity_bytes,
        )?;
        let paused = if run_root.path().join("intent.json").try_exists()? {
            let mut file = run_root
                .read_file("intent.json")
                .map_err(io::Error::other)?;
            let mut bytes = Vec::new();
            (&mut file).take(1025).read_to_end(&mut bytes)?;
            let intent: serde_json::Value = serde_json::from_slice(&bytes)?;
            if bytes.len() > 1024
                || intent.as_object().is_none_or(|v| v.len() != 3)
                || intent["schema"] != 1
                || intent["runId"] != run
            {
                return Err(invalid());
            }
            intent["paused"].as_bool().ok_or_else(invalid)?
        } else {
            false
        };
        cache.pause(paused);
        *state = Some(Runtime {
            cache,
            pins: opened.pins,
            run_root,
            source_bytes,
            paused,
            generation: std::process::id() as u64 * 1_000_000
                + std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| invalid())?
                    .as_millis() as u64
                    % 1_000_000,
        });
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn cache_runtime_size() -> u64 {
    state()
        .lock()
        .ok()
        .and_then(|state| state.as_ref().map(|runtime| runtime.source_bytes))
        .unwrap_or(0)
}
#[no_mangle]
pub extern "C" fn cache_runtime_port_token() -> u64 {
    state()
        .lock()
        .ok()
        .and_then(|state| state.as_ref().map(|runtime| runtime.pins.reservation.token))
        .unwrap_or(0)
}
/// # Safety
/// `buffer` points to `length` exclusively writable bytes for this call.
#[no_mangle]
pub unsafe extern "C" fn cache_runtime_read(offset: u64, buffer: *mut u8, length: u32) -> i32 {
    ffi(|| {
        with_runtime(|runtime| {
            bounds(runtime.source_bytes, offset, length as usize)?;
            if buffer.is_null() {
                return Err(invalid());
            }
            let output = unsafe { std::slice::from_raw_parts_mut(buffer, length as usize) };
            if runtime.cache.read_at(offset, output)? != output.len() {
                return Err(invalid());
            }
            Ok(())
        })
    })
}
/// # Safety
/// `buffer` points to `length` readable bytes for this synchronous call.
#[no_mangle]
pub unsafe extern "C" fn cache_runtime_write(offset: u64, buffer: *const u8, length: u32) -> i32 {
    ffi(|| {
        with_runtime(|runtime| {
            bounds(runtime.source_bytes, offset, length as usize)?;
            if buffer.is_null() {
                return Err(invalid());
            }
            let input = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
            if offset.is_multiple_of(4096) && input.len().is_multiple_of(4096) {
                runtime.cache.write_source(offset, input)?;
            } else {
                let mut completed = 0;
                while completed < input.len() {
                    let position = offset + completed as u64;
                    let aligned = position / 4096 * 4096;
                    let within = (position % 4096) as usize;
                    let count = (4096 - within).min(input.len() - completed);
                    let mut sector = [0; 4096];
                    if runtime.cache.read_at(aligned, &mut sector)? != sector.len() {
                        return Err(invalid());
                    }
                    sector[within..within + count]
                        .copy_from_slice(&input[completed..completed + count]);
                    runtime.cache.write_source(aligned, &sector)?;
                    completed += count;
                }
            }
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn cache_runtime_control(operation: u32) -> i32 {
    ffi(|| {
        with_runtime(|runtime| {
            match operation {
                0 => {}
                1 => {
                    runtime.cache.clear();
                    runtime.generation = runtime.generation.checked_add(1).ok_or_else(invalid)?;
                }
                2 => {
                    runtime.cache.pause(true);
                    runtime.paused = true;
                }
                3 => {
                    runtime.cache.pause(false);
                    runtime.paused = false;
                }
                _ => return Err(invalid()),
            }
            let value = serde_json::json!({"schema":1,"runId":runtime.pins.config.run,"sourceBytes":runtime.source_bytes,"paused":runtime.paused,"generation":runtime.generation,
            "capacityBytes":runtime.cache.capacity(),"occupancyBytes":runtime.cache.occupancy(),"statistics":runtime.cache.statistics(),
            "ssdPayloadProtected":true,"ssdFileExclusive":true,"everyWriteFlushedToSource":true,"cacheMappingsReused":false});
            let root: PathBuf = runtime.run_root.path().into();
            fs::write(root.join("stats.tmp"), serde_json::to_vec_pretty(&value)?)?;
            fs::rename(root.join("stats.tmp"), root.join("stats.json"))?;
            Ok(())
        })
    })
}
/// Caller first drains/offlines the projected filesystem, disconnects its native
/// session and stops target callbacks. Closing never onlines the source itself.
#[no_mangle]
pub extern "C" fn cache_runtime_close() -> i32 {
    ffi(|| {
        *state().lock().map_err(|_| invalid())? = None;
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_bounds_cover_real_hdd_addresses_and_reject_overflow() {
        let bytes = 2_000_398_934_016;
        assert!(bounds(bytes, bytes - 512, 512).is_ok());
        assert!(bounds(bytes, 1_999_999_500_288, 4096).is_ok());
        for (offset, length) in [
            (bytes, 512),
            (bytes - 512, 1024),
            (u64::MAX - 511, 512),
            (1, 512),
            (0, 513),
            (0, 0),
            (0, 1024 * 1024 + 512),
        ] {
            assert!(bounds(bytes, offset, length).is_err());
        }
    }
}
