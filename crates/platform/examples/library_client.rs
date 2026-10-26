#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use superhdd_platform::app_library::{self as lib, LibraryCommand};
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result=match args.as_slice(){
  [op] if op=="state"=>lib::state()?,
  [op] if op=="discover"=>lib::get_catalog(true)?,
  [op,file] if op=="config"=>{let command:lib::ConfigurationCommand=serde_json::from_slice(&std::fs::read(file)?)?;let snapshot=if command.operation=="save"{Some(superhdd_platform::discover()?)}else{None};lib::configuration_operation(command,snapshot)?},
  [op,file] if op=="operate"=>{let command:LibraryCommand=serde_json::from_slice(&std::fs::read(file)?)?;let snapshot=if command.operation=="start"{Some(superhdd_platform::discover()?)}else{None};lib::operate(command,snapshot)?},
  [op,path] if op=="add"=>lib::add(None,path.clone())?,
  [op,path,name] if op=="add"=>lib::add(Some(name.clone()),path.clone())?,
  [op,path] if op=="start"=>{let path=lib::normalize(path)?;lib::add(None,path.clone())?;lib::operate(LibraryCommand{operation:"start".into(),id:Some(lib::app_id(&path)),app:None,budget_bytes:None},Some(superhdd_platform::discover()?))?},
  [op,id] if ["stop","pause","resume","clear","remove","modeData","modeFull"].contains(&op.as_str())=>lib::operate(LibraryCommand{operation:op.clone(),id:Some(id.clone()),app:None,budget_bytes:None},None)?,
  [op,bytes] if op=="budget"=>lib::operate(LibraryCommand{operation:op.clone(),id:None,app:None,budget_bytes:Some(bytes.parse()?)},None)?,
  _=>return Err("library_client state | discover | add PATH [NAME] | start PATH | stop/pause/resume/clear/remove ID | budget BYTES".into())
 };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows required");
}
