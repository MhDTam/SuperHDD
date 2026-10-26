fn main() -> Result<(), Box<dyn std::error::Error>> {
    use superhdd_platform::cache_benchmark::{self as bench, Command};
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result=match args.as_slice(){[op]if op=="status"=>bench::status(),[op,config,profile]if op=="start"=>bench::execute(Command{action:"start".into(),configuration_id:Some(config.clone()),profile:Some(profile.clone()),workload:None},Some(superhdd_platform::discover()?))?,[op,config,profile,workload]if op=="start"=>bench::execute(Command{action:"start".into(),configuration_id:Some(config.clone()),profile:Some(profile.clone()),workload:Some(workload.clone())},Some(superhdd_platform::discover()?))?,[op]=>bench::execute(Command{action:op.clone(),configuration_id:None,profile:None,workload:None},None)?,_=>return Err("benchmark_client status | start CONFIG quick/standard | cancel | recover | openReport | openFolder".into())};
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
