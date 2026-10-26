//! Isolated-VM developer control. Installs nothing and writes no raw device.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!("raw_lab status | diagnose | configure REQUEST.json | pause | resume | clear | disable | bootstrap NEW_MARKER_FILE\nRequires an already installed, appropriately signed v4 driver. Configuration accepts distinct disposable VHD volumes only. Bootstrap creates a new regular file; it never writes a raw device.");
        return Ok(());
    }
    if let [command, file] = args.as_slice() {
        if command == "bootstrap" {
            use std::io::Write;
            let mut instance = [0u8; 16];
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            instance.copy_from_slice(&now.to_le_bytes());
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)?
                .write_all(&superhdd_platform::raw_lab::bootstrap(instance)?)?;
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        let driver = superhdd_platform::raw_lab::Controller::open()?;
        if args.as_slice() == ["diagnose"] {
            println!("{}", serde_json::to_string_pretty(&driver.diagnostic()?)?);
            return Ok(());
        }
        match args.as_slice() {
            [command] if command == "status" => {}
            [command, file] if command == "configure" => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(file)?
                    .take(8193)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 8192 {
                    return Err("Oversized RAW request".into());
                }
                driver.configure(&serde_json::from_slice(&bytes)?)?;
            }
            [command] if command == "pause" => driver.pause(true)?,
            [command] if command == "resume" => driver.pause(false)?,
            [command] if command == "clear" => driver.clear()?,
            [command] if command == "disable" => driver.disable()?,
            _ => return Err("Invalid command; use --help".into()),
        }
        println!("{}", serde_json::to_string_pretty(&driver.status()?)?);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Windows is required".into())
    }
}
