//! Mounted NTFS VM laboratory only. Installs nothing and changes no OS policy.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.is_empty() || arguments[0] == "--help" {
        println!("pnp_lab status | configure REQUEST.json | pause | resume | clear | disable\nRequires an already loaded, appropriately signed PnP lab driver in an isolated Windows VM. Source/cache must be distinct disposable FileBackedVirtual NTFS volumes. No driver, filter registration, partition, or security setting is installed or changed by this tool.");
        return Ok(());
    }
    #[cfg(windows)]
    {
        let driver = superhdd_platform::pnp_lab::Controller::open()?;
        match arguments.as_slice() {
            [command] if command == "status" => {}
            [command, file] if command == "configure" => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(file)?
                    .take(8193)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 8192 {
                    return Err("Oversized PnP lab request".into());
                }
                driver.configure(&serde_json::from_slice(&bytes)?)?;
            }
            [command] if command == "disable" => driver.disable()?,
            [command] if command == "pause" => driver.pause(true)?,
            [command] if command == "resume" => driver.pause(false)?,
            [command] if command == "clear" => driver.clear()?,
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
