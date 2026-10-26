//! Explicit administrator/developer mode; never exposed on the interactive-user
//! read-only pipe and never invoked automatically by desktop startup.
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
};
use superhdd_storage_control::{
    session::{Backend, Session},
    ContainerBudget,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    pub(crate) source_volume: String,
    pub(crate) container_root: PathBuf,
    pub(crate) budget_bytes: Option<u64>,
    pub(crate) acknowledged_disposable_source: bool,
    pub(crate) isolated_lab: Option<superhdd_storage_control::lab::LabIdentity>,
}
pub(crate) fn read_request(file: &Path) -> Result<Request, String> {
    let mut bytes = Vec::new();
    File::open(file)
        .map_err(|e| e.to_string())?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 8192 {
        return Err("Session request exceeds 8192 bytes.".into());
    }
    parse_request(&bytes)
}
fn parse_request(bytes: &[u8]) -> Result<Request, String> {
    let request: Request = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if !request.acknowledged_disposable_source || !request.container_root.is_absolute() {
        return Err("An absolute owned container directory and disposable-source acknowledgement are required.".into());
    }
    Ok(request)
}
fn reply<B: Backend>(session: &Session<B>, error: Option<String>) -> Result<(), String> {
    let target=session.target().map(|target|serde_json::json!({"sourceDisk":target.source_disk,"cacheDisk":target.cache_disk,"capacityBytes":target.capacity_bytes}));
    println!(
        "{}",
        serde_json::json!({"ok":error.is_none(),"phase":session.phase(),"target":target,"error":error,"scope":"Disposable-file-backed-source developer session; no physical activation."})
    );
    std::io::stdout().flush().map_err(|e| e.to_string())
}
pub fn run(file: &Path) -> Result<(), String> {
    let request = read_request(file)?;
    let budget = request.budget_bytes.map(ContainerBudget::new).transpose()?;
    // Driver/source validation happens before creating any backing. No driver is
    // installed, signed, or enrolled by this command.
    let mut session = match request.isolated_lab {
        None => superhdd_platform::managed_cache::open(
            &request.source_volume,
            &request.container_root,
            budget,
        )?,
        Some(identity) => superhdd_platform::managed_cache::open_isolated_lab(
            &request.source_volume,
            &request.container_root,
            budget,
            identity,
        )?,
    };
    session.start()?;
    reply(&session, None)?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = Vec::new();
        Read::by_ref(&mut input)
            .take(1025)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            break;
        }
        if line.len() > 1024 {
            return Err("Session command exceeds 1024 bytes.".into());
        }
        let command = std::str::from_utf8(&line)
            .map_err(|e| e.to_string())?
            .trim();
        let result = match command {
            "pause" => session.pause(true),
            "resume" => session.pause(false),
            "clear" => session.clear(),
            "stop" => {
                session.stop()?;
                reply(&session, None)?;
                return Ok(());
            }
            _ => Err("Use pause, resume, clear or stop.".into()),
        };
        reply(&session, result.err())?;
    }
    session.stop()?;
    reply(&session, None)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_requests_cannot_add_hidden_commands_or_skip_ownership_acknowledgement() {
        for bytes in [
            br#"{"sourceVolume":"D:","containerRoot":"M:\\owned","acknowledgedDisposableSource":true,"command":"format"}"#.as_slice(),
            br#"{"sourceVolume":"D:","containerRoot":"M:\\owned","acknowledgedDisposableSource":false}"#.as_slice(),
            br#"{"sourceVolume":"D:","containerRoot":"relative","acknowledgedDisposableSource":true}"#.as_slice(),
        ] {assert!(parse_request(bytes).is_err());}
    }
}
