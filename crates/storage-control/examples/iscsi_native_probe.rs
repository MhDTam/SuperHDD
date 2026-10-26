#[cfg(windows)]
fn main() {
    use superhdd_storage_control::iscsi;
    fn run() -> Result<serde_json::Value, String> {
        let args: Vec<_> = std::env::args().collect();
        if !(3..=4).contains(&args.len()) {
            return Err("Operation, owned run ID and optional volume required".into());
        }
        let managed = args[1].ends_with("-managed");
        let operation = args[1].strip_suffix("-managed").unwrap_or(&args[1]);
        let target = if managed {
            iscsi::managed_target_name(&args[2])?
        } else {
            iscsi::target_name(&args[2])?
        };
        if args[1] == "inspect" {
            let volume = superhdd_storage_control::windows::volume_identity(
                args.get(3).ok_or("Volume required")?,
            )?;
            let identity = superhdd_storage_control::data_source::inspect_volume(&volume)?;
            return Ok(
                serde_json::json!({"readOnly":true,"volume":volume,"disk":identity,"filesystem":superhdd_storage_control::windows::volume_filesystem(&volume.volume_guid)?}),
            );
        }
        if matches!(
            operation,
            "lock-source" | "restore-source" | "lock-target" | "restore-target"
        ) {
            return native_source::run(&args[1], &args[2], args.get(3).map(String::as_str));
        }
        if args.len() != 3 {
            return Err("Unexpected initiator argument".into());
        }
        match operation {
            "connect" => {
                let id = iscsi::connect(&target)?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                loop {
                    let devices = iscsi::devices(id)?;
                    if devices.len() == 1 {
                        return Ok(
                            serde_json::json!({"session":id,"target":target,"devices":devices,"persistent":false,"sendTargetPortalAdded":false}),
                        );
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err("Owned native LUN did not arrive".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            "disconnect" => {
                for session in iscsi::sessions()?
                    .into_iter()
                    .filter(|session| session.target == target)
                {
                    iscsi::disconnect(&target, session.id)?;
                }
                iscsi::remove_static_target(&target)?;
                Ok(serde_json::json!({"disconnected":true,"staticTargetRemoved":true}))
            }
            _ => Err("Unsupported native initiator operation".into()),
        }
    }
    match run() {
        Ok(result) => println!("{result}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows only");
    std::process::exit(1)
}
#[cfg(windows)]
#[path = "support/native_source.rs"]
mod native_source;
