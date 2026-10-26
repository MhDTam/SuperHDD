use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use superhdd_platform::downloads::Downloads;
type HttpFixture = (
    String,
    Arc<AtomicBool>,
    Arc<Mutex<Vec<u64>>>,
    thread::JoinHandle<()>,
    Vec<u8>,
);
fn fixture() -> HttpFixture {
    fixture_with_pause_gate(None)
}
fn fixture_with_pause_gate(pause_gate: Option<Arc<AtomicBool>>) -> HttpFixture {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let offsets = Arc::new(Mutex::new(vec![]));
    let logged = offsets.clone();
    let payload: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|i| ((i * 7 + i / 31) % 251) as u8)
        .collect();
    let body = payload.clone();
    let handle = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = vec![];
            let mut byte = [0u8; 1];
            while request.len() < 4096 {
                if stream.read_exact(&mut byte).is_err() {
                    break;
                }
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
            let offset = text
                .lines()
                .find_map(|line| {
                    line.strip_prefix("range: bytes=")
                        .and_then(|s| s.split('-').next())
                        .and_then(|s| s.parse::<usize>().ok())
                })
                .unwrap_or(0);
            logged.lock().unwrap().push(offset as u64);
            let header = if offset > 0 {
                format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nETag: \"fixture-v1\"\r\nConnection: close\r\n\r\n",body.len()-offset,offset,body.len()-1,body.len())
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"fixture-v1\"\r\nConnection: close\r\n\r\n",body.len())
            };
            if stream.write_all(header.as_bytes()).is_ok() {
                for (index, chunk) in body[offset..].chunks(16384).enumerate() {
                    if stream.write_all(chunk).is_err() {
                        break;
                    }
                    if offset == 0 && index == 3 {
                        // The pause test releases this after requesting pause.
                        // This guarantees a partial response even if its thread
                        // is descheduled long enough for a timed stream to end.
                        while pause_gate
                            .as_ref()
                            .is_some_and(|gate| gate.load(Ordering::SeqCst))
                            && !flag.load(Ordering::SeqCst)
                        {
                            thread::sleep(Duration::from_millis(1));
                        }
                    }
                    thread::sleep(Duration::from_millis(3));
                }
            }
        }
    });
    (
        format!("http://{address}/fixture.bin"),
        stop,
        offsets,
        handle,
        payload,
    )
}
#[test]
fn actual_http_pause_range_resume_and_verified_destination() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    let external_destination = std::env::var_os("SUPERHDD_TEST_HDD_DESTINATION").map(|path| {
        tempfile::Builder::new()
            .prefix("download-check-")
            .tempdir_in(path)
            .unwrap()
    });
    let destination = external_destination
        .as_ref()
        .map(|path| path.path().to_path_buf())
        .unwrap_or_else(|| dir.path().join("destination"));
    if external_destination.is_none() {
        fs::create_dir(&destination).unwrap();
    }
    let manager = Downloads::new(staging.clone()).unwrap();
    let pause_gate = Arc::new(AtomicBool::new(true));
    let (url, stop, offsets, server, payload) = fixture_with_pause_gate(Some(pause_gate.clone()));
    let job = manager.create(&url, &destination, 8 * 1024 * 1024).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while manager.list().unwrap()[0].bytes < 65536 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    manager.action(&job.id, "pause").unwrap();
    pause_gate.store(false, Ordering::SeqCst);
    loop {
        let list = manager.list().unwrap();
        let status = &list[0];
        if status.state == "paused" {
            assert!(status.bytes > 0);
            assert!(status.bytes < payload.len() as u64);
            break;
        }
        assert_ne!(status.state, "failed", "{:?}", status.error);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    loop {
        if manager.action(&job.id, "resume").is_ok() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    loop {
        let list = manager.list().unwrap();
        let status = &list[0];
        if status.state == "completed" {
            assert!(status.sha256.is_some());
            break;
        }
        assert_ne!(status.state, "failed", "{:?}", status.error);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read(destination.join("fixture.bin")).unwrap(), payload);
    eprintln!(
        "Verified download publication on {} ({} bytes)",
        destination.display(),
        payload.len()
    );
    assert!(offsets.lock().unwrap().iter().any(|offset| *offset > 0));
    assert!(!staging.join(format!("{}.part", job.id)).exists());
    assert!(manager.create(&url, &destination, 8 * 1024 * 1024).is_err());
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    drop(manager);
    assert_eq!(
        Downloads::new(staging).unwrap().list().unwrap()[0].state,
        "completed"
    );
}

#[test]
fn oversized_download_is_rejected_without_publishing_or_overrunning_quota() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    let destination = dir.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let manager = Downloads::new(staging.clone()).unwrap();
    let (url, stop, _, server, _) = fixture();
    let job = manager.create(&url, &destination, 65536).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = manager.list().unwrap().pop().unwrap();
        if state.state == "failed" {
            assert!(state.error.unwrap().contains("quota"));
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!destination.join("fixture.bin").exists());
    assert!(
        fs::metadata(staging.join(format!("{}.part", job.id)))
            .map(|m| m.len())
            .unwrap_or(0)
            <= 65536
    );
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
}

#[test]
fn interrupted_manifest_recovers_paused_with_actual_staged_length() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    fs::create_dir(&staging).unwrap();
    let id = "abc-123";
    let data = superhdd_platform::downloads::Download {
        id: id.into(),
        url: "http://127.0.0.1/test.bin".into(),
        name: "test.bin".into(),
        destination: dir.path().to_string_lossy().into(),
        bytes: 999999,
        total: Some(2000000),
        speed: 123,
        state: "downloading".into(),
        error: None,
        etag: Some("\"v1\"".into()),
        sha256: None,
        publish_ready: false,
        quota: 8000000,
    };
    fs::write(
        staging.join(format!("{id}.json")),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    fs::write(staging.join(format!("{id}.part")), [42u8; 4096]).unwrap();
    let recovered = Downloads::new(staging)
        .unwrap()
        .list()
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(recovered.state, "paused");
    assert_eq!(recovered.bytes, 4096);
    assert_eq!(recovered.speed, 0);
}
