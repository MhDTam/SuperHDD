//! Fixed local runtime package. Administrator installation and immutable binary
//! pins replace the custom-driver release contract for this separate backend.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};
pub const SERVICE: &str = "SuperHDDLoopbackService";
pub const LAB_SERVICE: &str = "SuperHDDLoopbackLab";
pub const FILES: [&str; 5] = [
    "SuperHDDService.exe",
    "SuperHDDCacheWorker.exe",
    "superhdd_loopback_runtime.dll",
    "ISCSI.dll",
    "DiskAccessLibrary.dll",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub protocol_version: u32,
    pub lab_only: bool,
    pub files: BTreeMap<String, String>,
}
pub struct Verified {
    pub root: PathBuf,
    pub manifest: Manifest,
    _directory: File,
    _manifest: File,
    _files: Vec<File>,
}
pub fn install_root() -> Result<PathBuf, String> {
    let old = crate::cache_runtime::install_root()?;
    Ok(old
        .parent()
        .ok_or("Missing Program Files directory")?
        .join("Super HDD Local Runtime"))
}
pub fn verify() -> Result<Verified, String> {
    let root = install_root()?;
    let directory = crate::cache_runtime::verify_install_directory(&root)?;
    let mut manifest_file =
        crate::cache_runtime::verify_file(&root.join("native-package.json"), "")?;
    manifest_file
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut manifest_file)
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("Oversized native runtime package".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if manifest.schema != 1 || manifest.protocol_version != 6 || manifest.files.len() != FILES.len()
    {
        return Err("Unknown native runtime package".into());
    }
    let mut pins = Vec::new();
    for name in FILES {
        let hash = manifest
            .files
            .get(name)
            .ok_or("Incomplete native runtime package")?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid native package hash".into());
        }
        pins.push(crate::cache_runtime::verify_file(&root.join(name), hash)?);
    }
    Ok(Verified {
        root,
        manifest,
        _directory: directory,
        _manifest: manifest_file,
        _files: pins,
    })
}
