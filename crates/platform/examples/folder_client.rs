#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use superhdd_platform::{
        cache_runtime::{Command, Operation},
        folder_runtime,
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result=match args.as_slice(){
        [action] if action=="state"=>folder_runtime::state(),
        [action,file] if action=="plan" || action=="start"=>{
            let request=serde_json::from_slice(&std::fs::read(file)?)?;
            let snapshot=superhdd_platform::discover()?;
            if action=="plan"{serde_json::to_value(folder_runtime::plan(&snapshot,request)?)?}
            else{folder_runtime::execute(Command{operation:Operation::Start,request:Some(request)},Some(snapshot))?}
        },
        [action]=>{
            let operation=match action.as_str(){"stop"=>Operation::Stop,"pause"=>Operation::Pause,"resume"=>Operation::Resume,"clear"=>Operation::Clear,"remove"=>Operation::Remove,_=>return Err("Unknown folder operation".into())};
            folder_runtime::execute(Command{operation,request:None},None)?
        },
        _=>return Err("folder_client state | plan REQUEST.json | start REQUEST.json | stop | pause | resume | clear | remove".into())
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows is required");
}
