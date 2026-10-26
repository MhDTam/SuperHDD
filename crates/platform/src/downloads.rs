//! Unprivileged managed downloads. Never run this URL/file worker as LocalSystem.
use reqwest::{
    blocking::Client,
    header::{CONTENT_RANGE, ETAG, IF_RANGE, RANGE},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub url: String,
    pub name: String,
    pub destination: String,
    pub bytes: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub state: String,
    pub error: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub publish_ready: bool,
    pub quota: u64,
}
struct Job {
    data: Download,
    pause: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}
#[derive(Clone)]
pub struct Downloads {
    root: PathBuf,
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    _owner: Arc<File>,
}
fn safe_name(url: &reqwest::Url) -> String {
    let raw = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or("download.bin");
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect::<String>()
        .trim_end_matches(['.', ' '])
        .to_string();
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    if cleaned.is_empty()
        || cleaned == "."
        || cleaned == ".."
        || matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
    {
        format!("file-{cleaned}.bin")
    } else {
        cleaned
    }
}
fn valid_range(header: &str, offset: u64) -> Option<u64> {
    let rest = header.strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start: u64 = start.parse().ok()?;
    let end: u64 = end.parse().ok()?;
    let total: u64 = total.parse().ok()?;
    if start == offset && end >= start && end < total {
        Some(total)
    } else {
        None
    }
}
impl Downloads {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn new(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".superhdd-owner.lock"))
            .map_err(|e| e.to_string())?;
        owner
            .try_lock()
            .map_err(|_| "Another process already owns this download folder.".to_string())?;
        let mut jobs = HashMap::new();
        for entry in fs::read_dir(&root).map_err(|e| e.to_string())?.flatten() {
            let filename = entry.file_name().to_string_lossy().into_owned();
            let pending_manifest = filename.ends_with(".json.new");
            if !filename.ends_with(".json") && !pending_manifest {
                continue;
            }
            if entry
                .metadata()
                .map(|m| m.len() > 1024 * 1024)
                .unwrap_or(true)
            {
                continue;
            }
            if let Ok(bytes) = fs::read(entry.path()) {
                if let Ok(mut data) = serde_json::from_slice::<Download>(&bytes) {
                    if data.id.is_empty()
                        || data.id.len() > 80
                        || !data.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                        || filename
                            != format!(
                                "{}.json{}",
                                data.id,
                                if pending_manifest { ".new" } else { "" }
                            )
                        || (pending_manifest && root.join(format!("{}.json", data.id)).exists())
                        || data.quota == 0
                        || data.quota > 1024u64.pow(4)
                        || !Path::new(&data.destination).is_absolute()
                        || !reqwest::Url::parse(&data.url)
                            .map(|url| {
                                matches!(url.scheme(), "http" | "https")
                                    && safe_name(&url) == data.name
                            })
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    if data.state != "completed" {
                        data.publish_ready |= data.state == "transferring";
                        data.state = "paused".into();
                        data.speed = 0;
                        data.bytes = fs::metadata(root.join(format!("{}.part", data.id)))
                            .map(|m| m.len())
                            .unwrap_or(0);
                    } else {
                        // Completion was durably recorded before removing our own payloads.
                        let _ = fs::remove_file(root.join(format!("{}.part", data.id)));
                        let _ = fs::remove_file(
                            Path::new(&data.destination)
                                .join(format!(".superhdd-{}.part", data.id)),
                        );
                    }
                    jobs.insert(
                        data.id.clone(),
                        Job {
                            data,
                            pause: Arc::new(AtomicBool::new(false)),
                            running: Arc::new(AtomicBool::new(false)),
                        },
                    );
                }
            }
        }
        Ok(Self {
            root,
            jobs: Arc::new(Mutex::new(jobs)),
            _owner: Arc::new(owner),
        })
    }
    pub fn list(&self) -> Result<Vec<Download>, String> {
        let guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
        let mut list: Vec<_> = guard.values().map(|j| j.data.clone()).collect();
        list.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(list)
    }
    fn update(&self, id: &str, change: impl FnOnce(&mut Download)) -> Result<(), String> {
        let mut guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
        let job = guard.get_mut(id).ok_or("Download not found")?;
        let mut updated = job.data.clone();
        change(&mut updated);
        self.persist(&updated)?;
        job.data = updated;
        Ok(())
    }
    fn persist(&self, data: &Download) -> Result<(), String> {
        let manifest = self.root.join(format!("{}.json", data.id));
        let pending = self.root.join(format!("{}.json.new", data.id));
        let bytes = serde_json::to_vec(data).map_err(|e| e.to_string())?;
        let mut file = File::create(&pending).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        // Atomic replacement of this manager-owned manifest (MoveFileExW on Windows).
        // Payload files are never overwritten.
        fs::rename(pending, manifest).map_err(|e| e.to_string())
    }
    pub fn create(&self, url: &str, destination: &Path, quota: u64) -> Result<Download, String> {
        let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid file URL")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Only HTTP and HTTPS URLs are supported.".into());
        }
        if quota == 0 || quota > 1024u64.pow(4) {
            return Err("Invalid staging quota.".into());
        }
        if !destination.is_absolute() || !destination.is_dir() {
            return Err("Choose an existing absolute destination folder.".into());
        }
        let destination = fs::canonicalize(destination).map_err(|e| e.to_string())?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let id = format!("{nanos:x}-{:x}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let data = Download {
            id: id.clone(),
            url: url.into(),
            name: safe_name(&parsed),
            destination: destination.to_string_lossy().into(),
            bytes: 0,
            total: None,
            speed: 0,
            state: "queued".into(),
            error: None,
            etag: None,
            sha256: None,
            publish_ready: false,
            quota,
        };
        if destination.join(&data.name).exists() {
            return Err("A destination file already exists; it will not be overwritten.".into());
        }
        {
            let mut guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
            if guard
                .values()
                .filter(|j| j.running.load(Ordering::SeqCst))
                .count()
                >= 4
            {
                return Err("At most four download workers may run at once.".into());
            }
            if guard
                .values()
                .filter(|j| j.data.state != "completed")
                .map(|j| j.data.bytes)
                .sum::<u64>()
                >= quota
            {
                return Err("The staging quota is already occupied.".into());
            }
            guard.insert(
                id.clone(),
                Job {
                    data: data.clone(),
                    pause: Arc::new(AtomicBool::new(false)),
                    running: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        self.update(&id, |_| {})?;
        self.resume(&id)?;
        Ok(data)
    }
    pub fn action(&self, id: &str, action: &str) -> Result<(), String> {
        match action {
            "remove" => {
                let mut guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
                let job = guard.get(id).ok_or("Download not found")?;
                if job.running.load(Ordering::SeqCst) {
                    return Err("Pause the worker and wait before removing this download.".into());
                }
                // Never remove the user's published destination file.
                for path in [
                    self.root.join(format!("{id}.part")),
                    self.root.join(format!("{id}.json.new")),
                    Path::new(&job.data.destination).join(format!(".superhdd-{id}.part")),
                ] {
                    if path.exists() {
                        fs::remove_file(path).map_err(|e| e.to_string())?;
                    }
                }
                fs::remove_file(self.root.join(format!("{id}.json"))).map_err(|e| e.to_string())?;
                guard.remove(id);
                Ok(())
            }
            "pause" => {
                let guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
                let job = guard.get(id).ok_or("Download not found")?;
                if job.data.state == "transferring" {
                    return Err("The verified destination transfer is in progress.".into());
                }
                job.pause.store(true, Ordering::SeqCst);
                Ok(())
            }
            "resume" => self.resume(id),
            _ => Err("Unsupported download action.".into()),
        }
    }
    fn resume(&self, id: &str) -> Result<(), String> {
        let (pause, running) = {
            let guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
            if guard
                .values()
                .filter(|job| job.running.load(Ordering::SeqCst))
                .count()
                >= 4
            {
                return Err("At most four download workers may run at once.".into());
            }
            let job = guard.get(id).ok_or("Download not found")?;
            if job.data.state == "completed" {
                return Err("Download is already complete.".into());
            }
            if job
                .running
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return Err("The download worker is still running; retry shortly.".into());
            }
            job.pause.store(false, Ordering::SeqCst);
            (job.pause.clone(), job.running.clone())
        };
        let manager = self.clone();
        let id = id.to_string();
        std::thread::spawn(move || {
            if let Err(error) = manager.run(&id, &pause) {
                let length = fs::metadata(manager.root.join(format!("{id}.part")))
                    .map(|m| m.len())
                    .unwrap_or(0);
                let _ = manager.update(&id, |job| {
                    job.state = "failed".into();
                    job.error = Some(error);
                    job.speed = 0;
                    job.bytes = length;
                });
            }
            running.store(false, Ordering::SeqCst);
        });
        Ok(())
    }
    fn run(&self, id: &str, pause: &AtomicBool) -> Result<(), String> {
        let data = self
            .list()?
            .into_iter()
            .find(|j| j.id == id)
            .ok_or("Download not found")?;
        let part = self.root.join(format!("{id}.part"));
        if data.publish_ready {
            return self.publish(id);
        }
        let existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut offset = if data.etag.is_some() && data.total.is_none_or(|total| existing < total) {
            existing
        } else {
            0
        };
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .timeout(std::time::Duration::from_secs(3600))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| e.to_string())?;
        let mut request = client.get(&data.url);
        if offset > 0 {
            request = request
                .header(RANGE, format!("bytes={offset}-"))
                .header(IF_RANGE, data.etag.clone().unwrap());
        }
        let response = request.send().map_err(|e| e.to_string())?;
        // A stale/out-of-range partial file must not trap the job in endless 416 retries.
        let response = if offset > 0 && response.status().as_u16() == 416 {
            offset = 0;
            client.get(&data.url).send().map_err(|e| e.to_string())?
        } else {
            response
        };
        let mut response = response.error_for_status().map_err(|e| e.to_string())?;
        let append = offset > 0 && response.status().as_u16() == 206;
        let total = if response.status().as_u16() == 206 {
            Some(
                valid_range(
                    response
                        .headers()
                        .get(CONTENT_RANGE)
                        .and_then(|h| h.to_str().ok())
                        .ok_or("Missing Content-Range")?,
                    if append { offset } else { 0 },
                )
                .ok_or("Invalid Content-Range; no data appended.")?,
            )
        } else if response.status().as_u16() == 200 {
            response.content_length()
        } else {
            return Err("Unexpected HTTP response status.".into());
        };
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|h| h.to_str().ok())
            .filter(|h| !h.starts_with("W/"))
            .map(str::to_string);
        if append && etag != data.etag {
            return Err(
                "The server changed the resume validator; existing staged bytes were retained."
                    .into(),
            );
        }
        if total.is_some_and(|n| n > data.quota) {
            return Err("The file exceeds the SSD staging quota.".into());
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!append)
            .append(append)
            .open(&part)
            .map_err(|e| e.to_string())?;
        let mut completed = if append { offset } else { 0 };
        self.update(id, |job| {
            job.state = "downloading".into();
            job.error = None;
            job.bytes = completed;
            job.total = total;
            job.etag = etag;
        })?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut tick = Instant::now();
        let mut tick_bytes = completed;
        loop {
            if pause.load(Ordering::SeqCst) {
                file.sync_all().map_err(|e| e.to_string())?;
                self.update(id, |job| {
                    job.state = "paused".into();
                    job.bytes = completed;
                    job.speed = 0;
                })?;
                return Ok(());
            }
            let length = response.read(&mut buffer).map_err(|e| e.to_string())?;
            if length == 0 {
                break;
            }
            // Reserve each chunk under the shared lock before writing, preventing
            // concurrent workers from racing past the global staging budget.
            {
                let mut guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
                let budget = guard
                    .values()
                    .filter(|j| j.data.state != "completed")
                    .map(|j| j.data.quota)
                    .min()
                    .unwrap_or(data.quota);
                let other_bytes: u64 = guard
                    .values()
                    .filter(|j| j.data.id != id && j.data.state != "completed")
                    .map(|j| j.data.bytes)
                    .sum();
                if completed
                    .saturating_add(length as u64)
                    .saturating_add(other_bytes)
                    > budget
                {
                    return Err("The total SSD staging quota has been reached.".into());
                }
                guard.get_mut(id).ok_or("Download not found")?.data.bytes =
                    completed + length as u64;
            }
            file.write_all(&buffer[..length])
                .map_err(|e| e.to_string())?;
            completed += length as u64;
            if tick.elapsed().as_secs_f64() >= 1.0 {
                let speed = ((completed - tick_bytes) as f64 / tick.elapsed().as_secs_f64()) as u64;
                self.update(id, |job| {
                    job.bytes = completed;
                    job.speed = speed;
                })?;
                tick = Instant::now();
                tick_bytes = completed;
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if total.is_some_and(|n| n != completed) {
            return Err("The downloaded length does not match the server response.".into());
        }
        self.update(id, |job| {
            job.bytes = completed;
            job.total = Some(completed);
            job.speed = 0;
            job.state = "transferring".into();
            job.publish_ready = true;
        })?;
        self.publish(id)
    }

    /// Recoverable publication. The original staged file remains authoritative
    /// until a verified destination and the completed manifest are durable.
    fn publish(&self, id: &str) -> Result<(), String> {
        let data = self
            .list()?
            .into_iter()
            .find(|job| job.id == id)
            .ok_or("Download not found")?;
        let part = self.root.join(format!("{id}.part"));
        let target = Path::new(&data.destination).join(&data.name);
        let pending = Path::new(&data.destination).join(format!(".superhdd-{id}.part"));
        let expected = if part.exists() {
            let (digest, length) = hash_file(&part)?;
            if data.total.is_some_and(|total| total != length) {
                return Err("Staged file length changed; publication stopped.".into());
            }
            if data.sha256.as_ref().is_some_and(|hash| hash != &digest) {
                return Err("Staged SHA-256 changed; publication stopped.".into());
            }
            digest
        } else {
            data.sha256
                .clone()
                .ok_or("Staged source is missing; no publication was attempted.")?
        };
        self.update(id, |job| {
            job.state = "transferring".into();
            job.publish_ready = true;
            job.sha256 = Some(expected.clone());
            job.speed = 0;
        })?;
        if target.exists() {
            if hash_file(&target)?.0 != expected {
                return Err(
                    "A different destination file exists; it will not be overwritten.".into(),
                );
            }
            return self.finish_publication(id, &part, &pending, &expected);
        }
        // This exact manager-owned temporary name may be left by an interrupted copy.
        if pending.exists() {
            fs::remove_file(&pending).map_err(|e| e.to_string())?;
        }
        let mut input = File::open(&part).map_err(|e| e.to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        let mut source_hash = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let length = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if length == 0 {
                break;
            }
            source_hash.update(&buffer[..length]);
            output
                .write_all(&buffer[..length])
                .map_err(|e| e.to_string())?;
        }
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        drop(input);
        if format!("{:x}", source_hash.finalize()) != expected || hash_file(&pending)?.0 != expected
        {
            return Err(
                "Destination SHA-256 verification failed; the staged source was retained.".into(),
            );
        }
        // NTFS hard-link creation is atomic and fails if target exists, avoiding a rename overwrite race.
        fs::hard_link(&pending, &target)
            .map_err(|e| format!("Destination publication failed; source retained: {e}"))?;
        self.finish_publication(id, &part, &pending, &expected)
    }

    fn finish_publication(
        &self,
        id: &str,
        part: &Path,
        pending: &Path,
        expected: &str,
    ) -> Result<(), String> {
        let mut guard = self.jobs.lock().map_err(|_| "Download lock unavailable")?;
        let job = guard.get_mut(id).ok_or("Download not found")?;
        let mut completed = job.data.clone();
        completed.state = "completed".into();
        completed.publish_ready = false;
        completed.error = None;
        completed.sha256 = Some(expected.into());
        completed.bytes = completed.total.unwrap_or(completed.bytes);
        self.persist(&completed)?;
        // A crash here is harmless: startup cleans completed jobs' owned payloads.
        let _ = fs::remove_file(pending);
        let _ = fs::remove_file(part);
        // Keep observers behind this lock until cleanup has been attempted.
        job.data = completed;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadSettings {
    pub root: String,
    pub warning: Option<String>,
}
/// Current-user location selection. Switching never moves or discards existing jobs.
pub struct DownloadStore {
    manager: Mutex<Downloads>,
    settings: PathBuf,
    warning: Mutex<Option<String>>,
}
impl DownloadStore {
    pub fn new(default_root: PathBuf, settings: PathBuf) -> Result<Self, String> {
        let configured = if settings.exists() {
            (|| {
                let mut bytes = Vec::new();
                File::open(&settings)
                    .map_err(|e| e.to_string())?
                    .take(4097)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 4096 {
                    return Err("Staging settings exceed 4096 bytes".into());
                }
                let saved: DownloadSettings =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                let path = PathBuf::from(saved.root);
                if !path.is_absolute() || !path.is_dir() {
                    return Err("Configured SSD staging folder is unavailable".into());
                }
                Downloads::new(path)
            })()
            .map(Some)
        } else {
            Ok(None)
        };
        let (manager, warning) = match configured {
            Ok(Some(manager)) => (manager, None),
            Ok(None) => (Downloads::new(default_root)?, None),
            Err(error) => (
                Downloads::new(default_root)?,
                Some(format!(
                    "{error}. Using the default staging folder; saved jobs were not moved."
                )),
            ),
        };
        Ok(Self {
            manager: Mutex::new(manager),
            settings,
            warning: Mutex::new(warning),
        })
    }
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, Downloads>, String> {
        self.manager
            .lock()
            .map_err(|_| "Staging location lock unavailable".into())
    }
    pub fn settings(&self) -> Result<DownloadSettings, String> {
        Ok(DownloadSettings {
            root: self.lock()?.root().to_string_lossy().into(),
            warning: self
                .warning
                .lock()
                .map_err(|_| "Staging warning lock unavailable")?
                .clone(),
        })
    }
    pub fn select_existing(&self, path: &Path) -> Result<DownloadSettings, String> {
        if !path.is_absolute() || !path.is_dir() {
            return Err("Choose an existing absolute SSD staging folder.".into());
        }
        let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut current = self.lock()?;
        let changing = current.root() != path;
        if changing && !current.list()?.is_empty() {
            return Err("Remove all existing download list entries before changing staging. Published files remain untouched.".into());
        }
        let replacement = if changing {
            Some(Downloads::new(path.clone())?)
        } else {
            None
        };
        let updated = DownloadSettings {
            root: path.to_string_lossy().into(),
            warning: None,
        };
        let pending = self.settings.with_extension("json.new");
        let mut file = File::create(&pending).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&updated).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&pending, &self.settings).map_err(|e| e.to_string())?;
        if let Some(replacement) = replacement {
            *current = replacement;
        }
        *self
            .warning
            .lock()
            .map_err(|_| "Staging warning lock unavailable")? = None;
        Ok(updated)
    }
}
/// Validate the actual canonical manager root, rather than LOCALAPPDATA or a UI label.
pub fn validate_staging(snapshot: &crate::Snapshot, root: &Path, quota: u64) -> Result<(), String> {
    let text = root
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .to_ascii_uppercase();
    let volume = snapshot
        .disks
        .iter()
        .filter(|disk| disk.kind == "SSD" && disk.online && !disk.read_only)
        .flat_map(|disk| &disk.volumes)
        .find(|volume| text.starts_with(&format!("{}\\", volume.letter.to_ascii_uppercase())))
        .ok_or("Staging must be on an identified online writable SSD.")?;
    if volume.file_system != "NTFS" || volume.health != "Healthy" {
        return Err("Choose a healthy NTFS SSD staging folder.".into());
    }
    if quota == 0
        || quota
            > volume
                .free
                .saturating_sub((volume.size / 10).max(5 * 1024u64.pow(3)))
    {
        return Err("Staging quota would violate the SSD free-space reserve.".into());
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut length = 0;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        length += count as u64;
    }
    Ok((format!("{:x}", digest.finalize()), length))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_range_must_match_the_resume_offset() {
        assert_eq!(valid_range("bytes 10-19/20", 10), Some(20));
        assert_eq!(valid_range("bytes 0-19/20", 10), None);
        assert_eq!(valid_range("bytes 10-20/20", 10), None);
        assert_eq!(valid_range("nonsense", 0), None)
    }
    #[test]
    fn urls_cannot_choose_windows_reserved_paths() {
        for path in ["CON", "NUL.txt", "..", "a%2Fb.exe"] {
            let name =
                safe_name(&reqwest::Url::parse(&format!("https://example.com/{path}")).unwrap());
            assert!(!name.contains('/'));
            assert!(!name.contains('\\'));
            assert_ne!(name, "CON");
            assert_ne!(name, "NUL.txt");
        }
    }
}
