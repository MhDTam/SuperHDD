//! Conservative discovery and validation of game data subtrees, never executables.
use std::{
    fs,
    path::{Path, PathBuf},
};

fn plain_dir(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
}
pub fn find(root: &Path) -> Option<PathBuf> {
    let mut bases = vec![root.to_path_buf()];
    let mut children: Vec<_> = fs::read_dir(root)
        .ok()?
        .take(64)
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| plain_dir(p))
        .filter(|p| {
            !matches!(
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "engine" | "binaries" | "redist" | "_redist" | "easyanticheat" | "battleye"
            )
        })
        .collect();
    children.sort();
    bases.extend(children);
    let mut candidates = vec![];
    for base in bases {
        for relative in ["Content\\Paks", "CookedPCConsole"] {
            let path = base.join(relative);
            // A currently owned data mount is a reparse point; catalog discovery
            // may describe it, while validate() below rejects any new such mount.
            if path.is_dir() {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 {
        candidates.pop()
    } else {
        None
    }
}
pub fn validate(root: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "libraryDataUnsupported")?;
    if relative.as_os_str().is_empty() {
        return Err("libraryDataUnsupported".into());
    }
    let mut ancestor = root.to_path_buf();
    if !plain_dir(root) {
        return Err("libraryDataUnsafe".into());
    }
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err("libraryDataUnsafe".into());
        }
        ancestor.push(part);
        if !plain_dir(&ancestor) {
            return Err("libraryDataUnsafe".into());
        }
    }
    let mut pending = vec![target.to_path_buf()];
    let mut count = 0;
    let mut assets = 0;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir).map_err(|_| "libraryDataUnsafe")? {
            count += 1;
            if count > 20000 {
                return Err("libraryDataUnsafe".into());
            }
            let path = entry.map_err(|_| "libraryDataUnsafe")?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| "libraryDataUnsafe")?;
            if metadata.file_attributes() & (0x400 | 0x4000) != 0 {
                return Err("libraryDataUnsafe".into());
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            let ext = path
                .extension()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                ext.as_str(),
                "exe" | "dll" | "sys" | "com" | "bat" | "cmd" | "ps1" | "msi" | "scr"
            ) {
                return Err("libraryDataUnsafe".into());
            }
            if matches!(ext.as_str(), "pak" | "ucas" | "utoc" | "upk" | "udk") {
                assets += 1;
            }
        }
    }
    if assets == 0 {
        return Err("libraryDataUnsupported".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_nested_paks_and_rejects_binaries() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("TslGame\\Content\\Paks");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("assets.pak"), b"data").unwrap();
        assert_eq!(find(temp.path()), Some(data.clone()));
        assert!(validate(temp.path(), &data).is_ok());
        fs::write(data.join("hidden.DLL"), b"binary").unwrap();
        assert_eq!(
            validate(temp.path(), &data).unwrap_err(),
            "libraryDataUnsafe"
        );
    }
    #[test]
    fn ambiguous_empty_and_outside_targets_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("Content\\Paks");
        let other = temp.path().join("Other\\Content\\Paks");
        fs::create_dir_all(&first).unwrap();
        assert_eq!(
            validate(temp.path(), &first).unwrap_err(),
            "libraryDataUnsupported"
        );
        fs::create_dir_all(&other).unwrap();
        assert!(find(temp.path()).is_none());
        assert!(validate(&first, &other).is_err());
        assert!(validate(temp.path(), temp.path()).is_err());
    }
}
