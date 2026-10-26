//! Developer-only controller. Never run attach against user data or outside a signed disposable-VM lab.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments[0] == "--help" {
        println!("driver_lab status | attach REQUEST.json | pause | resume | clear | detach\nAttach is VHD-only and requires a separately signed VM lab driver. Quiesce the raw test source before detach. No service or driver is installed by this utility.");
        return Ok(());
    }
    #[cfg(windows)]
    {
        let driver = superhdd_platform::driver::LabDriver::open()?;
        match arguments.as_slice() {
            [command] if command == "status" => {}
            [command, file] if command == "attach" => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(file)?
                    .take(8193)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 8192 {
                    return Err("Oversized lab request".into());
                }
                driver.attach(&serde_json::from_slice(&bytes)?)?;
            }
            [command] if command == "pause" => driver.pause(true)?,
            [command] if command == "resume" => driver.pause(false)?,
            [command] if command == "clear" => driver.clear()?,
            [command] if command == "detach" => driver.detach()?,
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
