use std::{fs, path::Path};
use superhdd_platform::{
    downloads::{validate_staging, DownloadSettings, DownloadStore, Downloads},
    Snapshot,
};
#[test]
fn selecting_the_fallback_folder_replaces_the_unavailable_saved_location() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("location.json");
    let default = dir.path().join("default");
    fs::write(
        &config,
        serde_json::to_vec(&DownloadSettings {
            root: dir.path().join("offline").to_string_lossy().into(),
            warning: None,
        })
        .unwrap(),
    )
    .unwrap();
    let store = DownloadStore::new(default.clone(), config.clone()).unwrap();
    assert!(store.settings().unwrap().warning.is_some());
    store.select_existing(&default).unwrap();
    assert!(store.settings().unwrap().warning.is_none());
    drop(store);
    let reloaded = DownloadStore::new(default, config).unwrap();
    assert!(reloaded.settings().unwrap().warning.is_none());
}
#[test]
fn selected_location_persists_and_old_owner_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("default");
    let selected = dir.path().join("ssd");
    fs::create_dir(&selected).unwrap();
    let config = dir.path().join("location.json");
    let store = DownloadStore::new(old.clone(), config.clone()).unwrap();
    let result = store.select_existing(&selected).unwrap();
    assert_eq!(
        Path::new(&result.root),
        fs::canonicalize(&selected).unwrap()
    );
    assert!(Downloads::new(old.clone()).is_ok());
    assert!(Downloads::new(selected.clone()).is_err());
    drop(store);
    let reopened = DownloadStore::new(old, config).unwrap();
    assert_eq!(reopened.settings().unwrap().root, result.root);
}
#[test]
fn selecting_location_never_discards_existing_download_entries() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("default");
    let selected = dir.path().join("ssd");
    fs::create_dir(&old).unwrap();
    fs::create_dir(&selected).unwrap();
    let manifest = serde_json::json!({"id":"abc-123","url":"http://127.0.0.1/file.bin","name":"file.bin","destination":selected,"bytes":0,"total":0,"speed":0,"state":"completed","error":null,"quota":1024});
    fs::write(
        old.join("abc-123.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let store = DownloadStore::new(old.clone(), dir.path().join("location.json")).unwrap();
    assert!(store.select_existing(&selected).is_err());
    assert_eq!(store.lock().unwrap().list().unwrap().len(), 1);
    assert!(old.join("abc-123.json").exists());
    assert!(!selected.join(".superhdd-owner.lock").exists());
}
#[test]
fn unavailable_or_malformed_saved_location_keeps_configuration_and_uses_default() {
    for oversized in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("location.json");
        let default = dir.path().join("default");
        let bytes = if oversized {
            vec![b' '; 4097]
        } else {
            serde_json::to_vec(&DownloadSettings {
                root: dir.path().join("missing").to_string_lossy().into(),
                warning: None,
            })
            .unwrap()
        };
        fs::write(&config, &bytes).unwrap();
        let store = DownloadStore::new(default.clone(), config.clone()).unwrap();
        assert_eq!(
            Path::new(&store.settings().unwrap().root),
            fs::canonicalize(&default).unwrap()
        );
        assert!(store.settings().unwrap().warning.is_some());
        assert_eq!(fs::read(config).unwrap(), bytes);
    }
}
#[test]
fn staging_reserve_uses_actual_selected_volume_and_rejects_hdd_unknown_and_offline() {
    let fixture = r#"{"disks":[{"number":0,"id":"ssd","model":"SSD","serial":"s","kind":"SSD","bus":"NVMe","size":1000000000000,"online":true,"readOnly":false,"system":false,"volumes":[{"letter":"M:","label":"SSD","fileSystem":"NTFS","size":1000000000000,"free":300000000000,"partitionNumber":1,"system":false,"health":"Healthy","encryption":"FullyDecrypted"}]}],"drivers":[],"serviceState":"NotInstalled","primoPath":null,"collectedAt":"test"}"#;
    let snapshot: Snapshot = serde_json::from_str(fixture).unwrap();
    assert!(validate_staging(&snapshot, Path::new(r"\\?\M:\stage"), 1024u64.pow(3)).is_ok());
    assert!(validate_staging(&snapshot, Path::new(r"C:\stage"), 1024u64.pow(3)).is_err());
    assert!(validate_staging(&snapshot, Path::new(r"M:\stage"), 250_000_000_000).is_err());
    for kind in ["HDD", "Unknown"] {
        let mut data = snapshot.clone();
        data.disks[0].kind = kind.into();
        assert!(validate_staging(&data, Path::new(r"M:\stage"), 1024).is_err());
    }
    let mut offline = snapshot.clone();
    offline.disks[0].online = false;
    assert!(validate_staging(&offline, Path::new(r"M:\stage"), 1024).is_err());
    let mut low = snapshot;
    low.disks[0].volumes[0].free = 50_000_000_000;
    assert!(validate_staging(&low, Path::new(r"M:\stage"), 1024).is_err());
}
