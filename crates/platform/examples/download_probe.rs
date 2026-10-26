//! Disposable process-lifetime probe; never installs a service or driver.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use superhdd_platform::downloads::Downloads;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("Usage: download_probe STAGING_DIRECTORY JOB_ID".into());
    }
    let manager = Downloads::new(PathBuf::from(&args[1]))?;
    let id = args[2].to_str().ok_or("Invalid job id")?;
    manager.action(id, "resume")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let job = manager
            .list()?
            .into_iter()
            .find(|job| job.id == id)
            .ok_or("Job missing")?;
        if job.state == "completed" {
            println!("{}", serde_json::to_string(&job)?);
            return Ok(());
        }
        if job.state == "failed" {
            return Err(job.error.unwrap_or("Download failed".into()).into());
        }
        if Instant::now() >= deadline {
            return Err("Probe timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
