use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use superhdd_platform::downloads::{Download, Downloads};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn payload() -> Vec<u8> {
    (0..256 * 1024)
        .map(|i| ((i * 31 + i / 97) % 251) as u8)
        .collect()
}
struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    destination: PathBuf,
    data: Download,
    bytes: Vec<u8>,
}
impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("stage");
        let destination = dir.path().join("destination");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&destination).unwrap();
        let bytes = payload();
        let data = Download {
            id: "abc-123".into(),
            url: "http://127.0.0.1:9/payload.bin".into(),
            name: "payload.bin".into(),
            destination: destination.to_string_lossy().into(),
            bytes: bytes.len() as u64,
            total: Some(bytes.len() as u64),
            speed: 0,
            state: "transferring".into(),
            error: None,
            etag: Some("\"old\"".into()),
            sha256: Some(hash(&bytes)),
            publish_ready: true,
            quota: 1024 * 1024,
        };
        Self {
            _dir: dir,
            root,
            destination,
            data,
            bytes,
        }
    }
    fn manifest(&self) {
        fs::write(
            self.root.join(format!("{}.json", self.data.id)),
            serde_json::to_vec(&self.data).unwrap(),
        )
        .unwrap();
    }
    fn part(&self) -> PathBuf {
        self.root.join(format!("{}.part", self.data.id))
    }
    fn pending(&self) -> PathBuf {
        self.destination
            .join(format!(".superhdd-{}.part", self.data.id))
    }
    fn target(&self) -> PathBuf {
        self.destination.join(&self.data.name)
    }
    fn wait(&self, manager: &Downloads) -> Download {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let state = manager
                .list()
                .unwrap()
                .into_iter()
                .find(|d| d.id == self.data.id)
                .unwrap();
            if matches!(state.state.as_str(), "completed" | "failed") {
                return state;
            }
            assert!(Instant::now() < deadline, "worker timeout");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn interrupted_destination_copy_is_rebuilt_from_verified_stage_without_network() {
    let setup = Setup::new();
    setup.manifest();
    fs::write(setup.part(), &setup.bytes).unwrap();
    fs::write(setup.pending(), [99; 1000]).unwrap();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    assert_eq!(manager.list().unwrap()[0].state, "paused");
    manager.action(&setup.data.id, "resume").unwrap();
    let result = setup.wait(&manager);
    assert_eq!(result.state, "completed", "{:?}", result.error);
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
    assert!(!setup.part().exists());
    assert!(!setup.pending().exists());
}
#[test]
fn crash_after_publish_and_before_manifest_recovers_without_stage() {
    let setup = Setup::new();
    setup.manifest();
    fs::write(setup.target(), &setup.bytes).unwrap();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    assert_eq!(setup.wait(&manager).state, "completed");
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
}
#[test]
fn recovery_never_replaces_a_different_destination_file() {
    let setup = Setup::new();
    setup.manifest();
    fs::write(setup.part(), &setup.bytes).unwrap();
    fs::write(setup.target(), b"user-owned existing file").unwrap();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    let result = setup.wait(&manager);
    assert_eq!(result.state, "failed");
    assert!(result.error.unwrap().contains("not be overwritten"));
    assert_eq!(
        fs::read(setup.target()).unwrap(),
        b"user-owned existing file"
    );
    assert!(setup.part().exists());
}
#[test]
fn changed_staging_hash_is_not_published() {
    let setup = Setup::new();
    setup.manifest();
    let mut corrupt = setup.bytes.clone();
    corrupt[17] ^= 255;
    fs::write(setup.part(), corrupt).unwrap();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    assert_eq!(setup.wait(&manager).state, "failed");
    assert!(!setup.target().exists());
    assert!(setup.part().exists());
}
#[test]
fn completed_manifest_reclaims_only_its_own_leftover_payload() {
    let mut setup = Setup::new();
    setup.data.state = "completed".into();
    setup.data.publish_ready = false;
    setup.manifest();
    fs::write(setup.part(), &setup.bytes).unwrap();
    fs::write(setup.pending(), &setup.bytes).unwrap();
    let other = setup.destination.join("keep.bin");
    fs::write(&other, b"keep").unwrap();
    let _manager = Downloads::new(setup.root.clone()).unwrap();
    assert!(!setup.part().exists());
    assert!(!setup.pending().exists());
    assert_eq!(fs::read(other).unwrap(), b"keep");
}

#[test]
fn removing_a_job_reclaims_staging_but_preserves_published_user_data() {
    let mut setup = Setup::new();
    setup.data.state = "completed".into();
    setup.data.publish_ready = false;
    setup.manifest();
    fs::write(setup.target(), &setup.bytes).unwrap();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "remove").unwrap();
    assert!(manager.list().unwrap().is_empty());
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
    assert!(!setup.root.join(format!("{}.json", setup.data.id)).exists());
}
#[test]
fn download_folder_has_one_owner_until_every_clone_is_dropped() {
    let setup = Setup::new();
    let first = Downloads::new(setup.root.clone()).unwrap();
    let clone = first.clone();
    assert!(Downloads::new(setup.root.clone()).is_err());
    drop(first);
    assert!(Downloads::new(setup.root.clone()).is_err());
    drop(clone);
    assert!(Downloads::new(setup.root.clone()).is_ok());
}
#[test]
fn initial_atomic_manifest_is_recoverable_and_path_manipulation_is_ignored() {
    let mut setup = Setup::new();
    setup.data.state = "downloading".into();
    setup.data.publish_ready = false;
    let pending = setup.root.join(format!("{}.json.new", setup.data.id));
    fs::write(&pending, serde_json::to_vec(&setup.data).unwrap()).unwrap();
    {
        let manager = Downloads::new(setup.root.clone()).unwrap();
        assert_eq!(manager.list().unwrap().len(), 1);
    }
    fs::remove_file(pending).unwrap();
    setup.data.name = "..\\do-not-write.bin".into();
    setup.manifest();
    let manager = Downloads::new(setup.root.clone()).unwrap();
    assert!(manager.list().unwrap().is_empty());
}

fn request(stream: &mut std::net::TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = vec![];
    let mut b = [0];
    while bytes.len() < 8192 && stream.read_exact(&mut b).is_ok() {
        bytes.push(b[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).to_ascii_lowercase()
}
fn network_setup(mode: &str) -> (Setup, thread::JoinHandle<Vec<String>>) {
    let mut setup = Setup::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    setup.data.url = format!("http://{}/payload.bin", listener.local_addr().unwrap());
    setup.data.state = "paused".into();
    setup.data.publish_ready = false;
    setup.data.sha256 = None;
    setup.data.bytes = 65536;
    fs::write(setup.part(), &setup.bytes[..65536]).unwrap();
    if mode == "weak" {
        setup.data.etag = None;
    }
    setup.manifest();
    let body = setup.bytes.clone();
    let mode = mode.to_string();
    let server = thread::spawn(move || {
        let count = if mode == "416" || mode == "redirect" {
            2
        } else {
            1
        };
        let mut requests = vec![];
        for index in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            requests.push(request(&mut stream));
            if mode == "416" && index == 0 {
                stream.write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                continue;
            }
            if mode == "redirect" && index == 0 {
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /real.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                continue;
            }
            let header = if mode == "changed" {
                format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 65536-{}/{}\r\nETag: \"changed\"\r\nConnection: close\r\n\r\n",body.len()-65536,body.len()-1,body.len())
            } else if mode == "truncated" {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"new\"\r\nConnection: close\r\n\r\n",body.len()*2)
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"new\"\r\nConnection: close\r\n\r\n",body.len())
            };
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(if mode == "changed" {
                &body[65536..]
            } else {
                &body
            });
        }
        requests
    });
    (setup, server)
}
#[test]
fn ignored_ranges_restart_cleanly_instead_of_appending() {
    let (setup, server) = network_setup("no-range");
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    assert_eq!(setup.wait(&manager).state, "completed");
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
    assert!(server.join().unwrap()[0].contains("range: bytes=65536-"));
}
#[test]
fn weak_validators_never_resume_old_bytes() {
    let (setup, server) = network_setup("weak");
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    assert_eq!(setup.wait(&manager).state, "completed");
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
    assert!(!server.join().unwrap()[0].contains("range:"));
}
#[test]
fn changing_etag_rejects_append_and_preserves_old_stage() {
    let (setup, server) = network_setup("changed");
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    let result = setup.wait(&manager);
    assert_eq!(result.state, "failed");
    assert!(result.error.unwrap().contains("validator"));
    assert_eq!(fs::read(setup.part()).unwrap(), setup.bytes[..65536]);
    assert!(!setup.target().exists());
    server.join().unwrap();
}
#[test]
fn range_416_restarts_once_from_zero() {
    let (setup, server) = network_setup("416");
    let manager = Downloads::new(setup.root.clone()).unwrap();
    manager.action(&setup.data.id, "resume").unwrap();
    assert_eq!(setup.wait(&manager).state, "completed");
    assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
    let requests = server.join().unwrap();
    assert!(requests[0].contains("range:"));
    assert!(!requests[1].contains("range:"));
}
#[test]
fn redirect_is_followed_and_truncation_is_never_published() {
    for mode in ["redirect", "truncated"] {
        let (setup, server) = network_setup(mode);
        let manager = Downloads::new(setup.root.clone()).unwrap();
        manager.action(&setup.data.id, "resume").unwrap();
        let state = setup.wait(&manager);
        if mode == "redirect" {
            assert_eq!(state.state, "completed");
            assert_eq!(fs::read(setup.target()).unwrap(), setup.bytes);
        } else {
            assert_eq!(state.state, "failed");
            assert!(!setup.target().exists());
        }
        server.join().unwrap();
    }
}
#[test]
fn resuming_saved_jobs_cannot_exceed_four_live_workers() {
    let setup = Setup::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/payload.bin", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let server = thread::spawn(move || {
        let mut clients = vec![];
        while !flag.load(Ordering::SeqCst) {
            if let Ok((client, _)) = listener.accept() {
                clients.push(client);
            }
            thread::sleep(Duration::from_millis(5));
        }
    });
    for i in 0..5 {
        let mut job = setup.data.clone();
        job.id = format!("abc-{i}");
        job.state = "paused".into();
        job.publish_ready = false;
        job.bytes = 0;
        job.url = url.clone();
        job.sha256 = None;
        fs::write(
            setup.root.join(format!("{}.json", job.id)),
            serde_json::to_vec(&job).unwrap(),
        )
        .unwrap();
    }
    let manager = Downloads::new(setup.root.clone()).unwrap();
    for i in 0..4 {
        manager.action(&format!("abc-{i}"), "resume").unwrap();
    }
    assert!(manager
        .action("abc-4", "resume")
        .unwrap_err()
        .contains("four"));
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while manager
        .list()
        .unwrap()
        .iter()
        .filter(|job| job.state == "failed")
        .count()
        < 4
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
}
