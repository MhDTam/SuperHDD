use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(windows)]
mod direct_host;
#[cfg(windows)]
mod transparent_host;
#[cfg(windows)]
mod transparent_ops;
#[cfg(windows)]
mod paging_host;
#[cfg(windows)]
mod paging_ops;
#[cfg(windows)]
mod lab_control;
#[cfg(windows)]
mod managed_host;
#[cfg(windows)]
mod runtime_ops;
#[cfg(windows)]
mod loopback_lab;
#[cfg(windows)]
mod loopback_host;
#[cfg(windows)]
mod loopback_ops;
#[cfg(windows)]
mod loopback_scm;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|s=="--paging-operation") {
        let result=if args.len()==3 {superhdd_platform::cache_runtime::Command::decode(&args[2]).and_then(paging_ops::run)}else{Err("One encoded filter operation is required".into())};
        if let Err(error)=result {eprintln!("{error}");std::process::exit(if error=="runtimeRestartRequired"{23}else{1});}return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|matches!(s.as_str(),"--prepare-paging-lab"|"--remove-paging-lab")) {
        let result=if args.len()==3 {if args[1]=="--prepare-paging-lab"{paging_host::prepare_lab(std::path::Path::new(&args[2]))}else{paging_host::remove_lab(&args[2])}}else{Err("One exact owned VM request is required".into())};
        if let Err(error)=result {eprintln!("{error}");std::process::exit(1);}return;
    }
    if args.len()==2 && args[1]=="--paging-release-info" {
        println!("{}",serde_json::json!({"protocolVersion":8,"backend":"storageFilter","driverSha256":superhdd_platform::paging_runtime::qualified_driver_hash(),"runtimeQualified":superhdd_platform::paging_runtime::qualified_driver_hash().is_some()}));return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|s=="--transparent-operation") {
        let result=if args.len()==3 {superhdd_platform::cache_runtime::Command::decode(&args[2]).and_then(transparent_ops::run)}else{Err("One encoded filter operation is required".into())};
        if let Err(error)=result {eprintln!("{error}");std::process::exit(if error=="runtimeRestartRequired"{23}else{1});}return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|matches!(s.as_str(),"--prepare-transparent-lab"|"--remove-transparent-lab")) {
        let result=if args.len()==3 {if args[1]=="--prepare-transparent-lab"{transparent_host::prepare_lab(std::path::Path::new(&args[2]))}else{transparent_host::remove_lab(&args[2])}}else{Err("One exact owned VM request is required".into())};
        if let Err(error)=result {eprintln!("{error}");std::process::exit(1);}return;
    }
    if args.len()==2 && args[1]=="--transparent-release-info" {
        println!("{}",serde_json::json!({"protocolVersion":7,"backend":"storageFilter","driverSha256":superhdd_platform::transparent_runtime::qualified_driver_hash(),"runtimeQualified":superhdd_platform::transparent_runtime::qualified_driver_hash().is_some()}));return;
    }
    #[cfg(windows)]
    if args.len()==2 && matches!(args[1].as_str(),"--loopback-lab-status"|"--loopback-status"|"--loopback-telemetry") {
        let result=if args[1]=="--loopback-telemetry"{superhdd_platform::ipc::native_telemetry().and_then(|s|serde_json::to_value(s).map_err(|e|e.to_string()))}else{superhdd_platform::ipc::native_status(args[1]=="--loopback-lab-status")};
        match result{Ok(status)=>println!("{status}"),Err(error)=>{eprintln!("{error}");std::process::exit(1);}}return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|s=="--loopback-operation") {
        let result=if args.len()==3{superhdd_platform::cache_runtime::Command::decode(&args[2]).and_then(loopback_ops::run)}else{Err("One encoded native cache operation required".into())};
        if let Err(error)=result{eprintln!("{error}");let code=match error.as_str(){"runtimeSourceBusy"=>20,"selectedScopeUnavailable"=>21,"runtimeOperationPending"=>22,_=>1};std::process::exit(code);}return;
    }
    #[cfg(windows)]
    if args.len()==2 && args[1]=="--verify-loopback-package" {
        match superhdd_platform::native_package::verify(){Ok(package)=>println!("{}",serde_json::json!({"verified":true,"labOnly":package.manifest.lab_only})),Err(error)=>{eprintln!("{error}");std::process::exit(1);}}
        return;
    }
    #[cfg(windows)]
    if args.len()==2 && matches!(args[1].as_str(),"--loopback-lab-service"|"--loopback-service") {
        if let Err(error)=loopback_host::start_scm(args[1]=="--loopback-lab-service"){eprintln!("{error}");std::process::exit(1);}
        return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|matches!(s.as_str(),"--prepare-loopback-lab"|"--remove-loopback-lab")) {
        let result=if args.len()==3 {
            if args[1]=="--prepare-loopback-lab" {loopback_lab::prepare(&args[2])}else{loopback_lab::remove(&args[2])}
        }else{Err("One owned disposable run ID required".into())};
        match result{Ok(value)=>println!("{value}"),Err(error)=>{eprintln!("{error}");std::process::exit(1)}}return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s|s=="--loopback-preflight") {
        let result=(||->Result<serde_json::Value,String>{
            if args.len()!=3 {return Err("One encoded cache request required".into());}
            let command=superhdd_platform::cache_runtime::Command::decode(&args[2])?;
            let request=command.request.ok_or("A start request is required for read-only native preflight")?;
            let context=superhdd_platform::loopback_cache::Context::capture(request)?;
            Ok(serde_json::json!({"backend":"windowsInitiator","preflightPassed":true,"readOnly":true,"active":false,"context":context}))
        })();
        match result {Ok(value)=>println!("{value}"),Err(error)=>{eprintln!("{error}");std::process::exit(1)}}
        return;
    }
    if args.len() == 2 && args[1] == "--release-info" {
        println!(
            "{}",
            serde_json::json!({"protocolVersion":5,"driverSha256":superhdd_platform::cache_runtime::qualified_driver_hash(),"runtimeQualified":superhdd_platform::cache_runtime::qualified_driver_hash().is_some()})
        );
        return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|s| s == "--cache-operation") {
        let result = if args.len() == 3 {
            superhdd_platform::cache_runtime::Command::decode(&args[2]).and_then(runtime_ops::run)
        } else {
            Err("One exact cache operation is required".into())
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1)
        }
        return;
    }
    #[cfg(windows)]
    if args
        .get(1)
        .is_some_and(|s| matches!(s.as_str(), "--prepare-direct-lab" | "--remove-direct-lab"))
    {
        let result = match (args.get(1).map(String::as_str), args.get(2), args.len()) {
            (Some("--prepare-direct-lab"), Some(path), 3) => {
                direct_host::prepare_lab(std::path::Path::new(path))
            }
            (Some("--remove-direct-lab"), Some(run), 3) => direct_host::remove_lab(run),
            _ => Err("Use one exact request or owned lab run ID".into()),
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1)
        }
        return;
    }
    #[cfg(windows)]
    if args
        .get(1)
        .is_some_and(|a| matches!(a.as_str(), "--prepare-managed-lab" | "--remove-managed-lab"))
    {
        let result = match args.as_slice() {
            [_, command, value] if command == "--prepare-managed-lab" => {
                managed_host::prepare(std::path::Path::new(value))
            }
            [_, command, value] if command == "--remove-managed-lab" => managed_host::remove(value),
            _ => {
                Err("Use --prepare-managed-lab REQUEST.json or --remove-managed-lab RUN_ID".into())
            }
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|arg| arg == "--lab-cache-session") {
        let result = if let [_, _, path] = args.as_slice() {
            lab_control::run(std::path::Path::new(path))
        } else {
            Err("Use --lab-cache-session REQUEST.json".into())
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--diagnostics") {
        match superhdd_platform::discover() {
            Ok(snapshot) => println!("{}", serde_json::to_string_pretty(&snapshot).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1)
            }
        }
        return;
    }
    #[cfg(windows)]
    if let Err(error) = windows_host::start(
        args.len() == 2 && args[1] == "--managed-cache-service",
        args.len() == 2 && args[1] == "--managed-direct-service",
        args.len() == 2 && args[1] == "--managed-transparent-service",
        args.len() == 2 && args[1] == "--managed-paging-service",
    ) {
        eprintln!("{error}");
        std::process::exit(1)
    }
    #[cfg(not(windows))]
    {
        eprintln!("Windows services are only supported on Windows.");
        std::process::exit(1)
    }
}

#[cfg(windows)]
mod windows_host {
    use super::*;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };
    define_windows_service!(service_entry, service_main);
    static MANAGED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static DIRECT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static TRANSPARENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static PAGING: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    enum DirectOwner { Legacy(crate::direct_host::DirectHost), Transparent(crate::transparent_host::TransparentHost), Paging(crate::paging_host::PagingHost) }
    impl DirectOwner {
        fn start()->Result<Self,String>{if PAGING.get()==Some(&true){crate::paging_host::PagingHost::start().map(Self::Paging)}else if TRANSPARENT.get()==Some(&true){crate::transparent_host::TransparentHost::start().map(Self::Transparent)}else{crate::direct_host::DirectHost::start().map(Self::Legacy)}}
        fn state(&self)->serde_json::Value{match self{Self::Legacy(o)=>o.state(),Self::Transparent(o)=>o.state(),Self::Paging(o)=>o.state()}}
        fn action(&mut self,a:superhdd_platform::ipc::CacheAction)->Result<serde_json::Value,String>{match self{Self::Legacy(o)=>o.action(a),Self::Transparent(o)=>o.action(a),Self::Paging(o)=>o.action(a)}}
        fn stop(&mut self)->Result<(),String>{match self{Self::Legacy(o)=>o.stop(),Self::Transparent(o)=>o.stop(),Self::Paging(o)=>o.stop()}}
    }
    pub fn start(managed: bool, direct: bool, transparent:bool, paging:bool) -> Result<(), windows_service::Error> {
        let _ = MANAGED.set(managed);
        let _ = DIRECT.set(direct);
        let _ = TRANSPARENT.set(transparent);
        let _ = PAGING.set(paging);
        service_dispatcher::start("SuperHDDService", service_entry)
    }
    fn service_main(_args: Vec<std::ffi::OsString>) {
        if let Err(error) = run() {
            eprintln!("{error}")
        }
    }
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let status =
            service_control_handler::register("SuperHDDService", move |event| match event {
                ServiceControl::Stop => {
                    flag.store(true, Ordering::SeqCst);
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            })?;
        let set_state = |state, accepted, exit_code, checkpoint, wait_hint| {
            status.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: accepted,
                exit_code,
                checkpoint,
                wait_hint,
                process_id: None,
            })
        };
        let zero = ServiceExitCode::Win32(0);
        set_state(
            ServiceState::StartPending,
            ServiceControlAccept::empty(),
            zero,
            1,
            std::time::Duration::from_secs(45),
        )?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if DIRECT.get() == Some(&true) || TRANSPARENT.get()==Some(&true) || PAGING.get()==Some(&true) {
                let state = Arc::new(std::sync::Mutex::new(serde_json::Value::Null));
                let admin_state = state.clone();
                let admin_stop = stop.clone();
                let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
                let admin = std::thread::spawn(move || -> Result<(), String> {
                    // Native VHD handles and the session stay on this one owner thread.
                    let mut owner = match DirectOwner::start() {
                        Ok(owner) => owner,
                        Err(e) => {
                            let _ = ready_tx.send(Err(e.clone()));
                            return Err(e);
                        }
                    };
                    *admin_state.lock().map_err(|_| "State unavailable")? = owner.state();
                    let _ = ready_tx.send(Ok(()));
                    let result = superhdd_platform::ipc::serve_admin(
                        admin_stop.clone(),
                        &superhdd_platform::DiscoveryCache::default(),
                        |request| {
                            if request.get("command").and_then(serde_json::Value::as_str)
                                != Some("cache_action")
                            {
                                return None;
                            }
                            let result =
                                serde_json::from_value::<superhdd_platform::ipc::CacheAction>(
                                    request.get("action").cloned().unwrap_or_default(),
                                )
                                .map_err(|e| e.to_string())
                                .and_then(|a| owner.action(a));
                            if let Ok(mut state) = admin_state.lock() {
                                *state = owner.state();
                            }
                            Some(match result {
                                Ok(s) => serde_json::json!({"ok":true,"result":s}),
                                Err(e) => serde_json::json!({"ok":false,"error":e}),
                            })
                        },
                    );
                    admin_stop.store(true, Ordering::SeqCst);
                    owner.stop()?;
                    if let Ok(mut state) = admin_state.lock() {
                        *state = owner.state();
                    }
                    result
                });
                ready_rx
                    .recv()
                    .map_err(std::io::Error::other)?
                    .map_err(std::io::Error::other)?;
                set_state(
                    ServiceState::Running,
                    ServiceControlAccept::STOP,
                    ServiceExitCode::Win32(0),
                    0,
                    std::time::Duration::ZERO,
                )?;
                let public_result = superhdd_platform::ipc::serve_with_status(
                    stop.clone(),
                    &superhdd_platform::DiscoveryCache::default(),
                    |request| match request.get("command").and_then(serde_json::Value::as_str) {
                        Some("cache_status") => Some(match state.lock() {
                            Ok(s) => serde_json::json!({"ok":true,"result":s.clone()}),
                            Err(_) => serde_json::json!({"ok":false,"error":"State unavailable"}),
                        }),
                        Some("telemetry") => {
                            let telemetry=if PAGING.get()==Some(&true){superhdd_platform::paging_driver::try_telemetry().map(|s|s.map(|v|v.cache))}else if TRANSPARENT.get()==Some(&true){superhdd_platform::transparent_driver::try_telemetry().map(|s|s.map(|v|v.cache))}else{superhdd_platform::direct_driver::try_telemetry().map(|s|s.map(|v|v.cache))};
                            Some(match telemetry {
                                Ok(Some(s)) => serde_json::json!({"ok":true,"result":s}),
                                Ok(None) => {
                                    serde_json::json!({"ok":false,"error":"Native v5 unavailable"})
                                }
                                Err(e) => serde_json::json!({"ok":false,"error":e}),
                            })
                        }
                        _ => None,
                    },
                );
                stop.store(true, Ordering::SeqCst);
                set_state(
                    ServiceState::StopPending,
                    ServiceControlAccept::empty(),
                    ServiceExitCode::Win32(0),
                    1,
                    std::time::Duration::from_secs(30),
                )?;
                let admin_result = admin
                    .join()
                    .map_err(|_| std::io::Error::other("Admin pipe thread failed"))?;
                public_result.map_err(std::io::Error::other)?;
                admin_result.map_err(std::io::Error::other)?;
                return Ok(());
            }
            let mut managed = if MANAGED.get() == Some(&true) {
                Some(crate::managed_host::ManagedHost::start().map_err(std::io::Error::other)?)
            } else {
                None
            };
            set_state(
                ServiceState::Running,
                ServiceControlAccept::STOP,
                ServiceExitCode::Win32(0),
                0,
                std::time::Duration::ZERO,
            )?;
            let cache = superhdd_platform::DiscoveryCache::default();
            let server_result = superhdd_platform::ipc::serve(stop.clone(), &cache);
            set_state(
                ServiceState::StopPending,
                ServiceControlAccept::empty(),
                ServiceExitCode::Win32(0),
                1,
                std::time::Duration::from_secs(30),
            )?;
            if let Some(owner) = &mut managed {
                owner.stop().map_err(std::io::Error::other)?;
            }
            server_result.map_err(std::io::Error::other)?;
            Ok(())
        })();
        set_state(
            ServiceState::Stopped,
            ServiceControlAccept::empty(),
            if result.is_ok() {
                ServiceExitCode::Win32(0)
            } else {
                ServiceExitCode::ServiceSpecific(1)
            },
            0,
            std::time::Duration::ZERO,
        )?;
        result
    }
}
