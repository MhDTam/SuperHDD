#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use superhdd_platform::{CachePlan, CacheRequest, DiscoveryCache, Snapshot};
#[tauri::command]
async fn benchmark_status()->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(superhdd_platform::cache_benchmark::status).await.map_err(|e|e.to_string())}
#[tauri::command]
async fn benchmark_action(command:superhdd_platform::cache_benchmark::Command,state:tauri::State<'_,DiscoveryCache>)->Result<serde_json::Value,String>{let snapshot=if command.action=="start"{Some(state.get(true)?)}else{None};tauri::async_runtime::spawn_blocking(move||superhdd_platform::cache_benchmark::execute(command,snapshot)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn get_app_library(refresh:bool)->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(move||superhdd_platform::app_library::get_catalog(refresh)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn app_library_operation(command:superhdd_platform::app_library::LibraryCommand,state:tauri::State<'_,DiscoveryCache>)->Result<serde_json::Value,String>{let snapshot=if command.operation=="start"{Some(state.get(true)?)}else{None};tauri::async_runtime::spawn_blocking(move||superhdd_platform::app_library::operate(command,snapshot)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn cache_configuration_operation(command:superhdd_platform::app_library::ConfigurationCommand,state:tauri::State<'_,DiscoveryCache>)->Result<serde_json::Value,String>{let snapshot=if command.operation=="save"{Some(state.get(true)?)}else{None};tauri::async_runtime::spawn_blocking(move||superhdd_platform::app_library::configuration_operation(command,snapshot)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn add_library_folder(name:Option<String>,path:String)->Result<serde_json::Value,String>{tauri::async_runtime::spawn_blocking(move||superhdd_platform::app_library::add(name,path)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn pick_app_folder()->Result<Option<String>,String>{tauri::async_runtime::spawn_blocking(superhdd_platform::app_library::pick_folder).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn get_folder_runtime() -> Result<serde_json::Value,String> {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(||superhdd_platform::app_library::aggregate().unwrap_or_else(superhdd_platform::folder_runtime::state)).await.map_err(|e|e.to_string());
    #[cfg(not(windows))]
    Err("desktopRequired".into())
}
#[tauri::command]
async fn folder_operation(command:superhdd_platform::cache_runtime::Command,state:tauri::State<'_,DiscoveryCache>)->Result<serde_json::Value,String>{
    if matches!(command.operation,superhdd_platform::cache_runtime::Operation::Start)!=command.request.is_some(){return Err("Invalid folder operation".into())}
    #[cfg(windows)] {
        let snapshot=if command.request.is_some(){Some(state.get(true)?)}else{None};
        tauri::async_runtime::spawn_blocking(move||{
            if let Some(request)=command.request {return superhdd_platform::app_library::start_request(request,snapshot.ok_or("Missing devices")?)}
            if superhdd_platform::app_library::aggregate().is_some(){let op=match command.operation{superhdd_platform::cache_runtime::Operation::Stop=>"stop",superhdd_platform::cache_runtime::Operation::Pause=>"pause",superhdd_platform::cache_runtime::Operation::Resume=>"resume",superhdd_platform::cache_runtime::Operation::Clear=>"clear",_=>return Err("Use library controls".into())};return superhdd_platform::app_library::control_all(op)}
            superhdd_platform::folder_runtime::execute(command,snapshot)
        }).await.map_err(|e|e.to_string())?
    }
    #[cfg(not(windows))]
    Err("desktopRequired".into())
}
#[tauri::command]
async fn get_cache_runtime() -> Result<superhdd_platform::cache_runtime::RuntimeState, String> {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(superhdd_platform::cache_runtime::state)
        .await
        .map_err(|e| e.to_string());
    #[cfg(not(windows))]
    Err("desktopRequired".into())
}
#[tauri::command]
async fn cache_operation(
    command: superhdd_platform::cache_runtime::Command,
) -> Result<superhdd_platform::cache_runtime::RuntimeState, String> {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(move || {
        superhdd_platform::cache_runtime::execute(command)
    })
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    Err("desktopRequired".into())
}
#[tauri::command]
async fn get_snapshot(
    state: tauri::State<'_, DiscoveryCache>,
    force: bool,
) -> Result<Snapshot, String> {
    #[cfg(windows)]
    if let Ok(snapshot) = superhdd_platform::ipc::snapshot(force) {
        return Ok(snapshot);
    }
    state.get(force)
}
#[tauri::command]
async fn plan_cache(
    request: CacheRequest,
    state: tauri::State<'_, DiscoveryCache>,
) -> Result<CachePlan, String> {
    #[cfg(windows)]
    if request.scope.is_some(){return superhdd_platform::folder_runtime::plan(&state.get(true)?,request)}
    superhdd_platform::plan_cache(&state.get(true)?, request)
}
#[tauri::command]
async fn get_cache_telemetry() -> Result<superhdd_platform::driver::Telemetry, String> {
    tauri::async_runtime::spawn_blocking(|| {
    #[cfg(windows)]
    if let Some(value)=superhdd_platform::app_library::telemetry(){return Ok(value)}
    #[cfg(windows)]
    if let Some(value)=superhdd_platform::folder_runtime::telemetry(){return Ok(value)}
    #[cfg(windows)]
    if superhdd_platform::native_runtime::state().is_some_and(|s|matches!(s.phase.as_str(),"active"|"paused"|"faulted")){return superhdd_platform::ipc::native_telemetry();}
    #[cfg(windows)]
    if let Ok(stats) = superhdd_platform::ipc::telemetry() {
        return Ok(stats);
    }
    superhdd_platform::driver::telemetry()
    }).await.map_err(|e|e.to_string())?
}
fn main() {
    let Some(_instance) = superhdd_platform::instance::claim_desktop().unwrap_or_else(|error| {
        eprintln!("Cannot initialize Super HDD: {error}");
        std::process::exit(1);
    }) else {
        return;
    };
    tauri::Builder::default()
        .setup(|_| {
            #[cfg(windows)]
            std::thread::spawn(|| {
                if let Err(error) = superhdd_platform::app_library::initialize_recovery() {
                    eprintln!("Folder recovery needs attention: {error}");
                }
            });
            Ok(())
        })
        .manage(DiscoveryCache::default())
        .invoke_handler(tauri::generate_handler![
            benchmark_status,
            benchmark_action,
            get_app_library,
            app_library_operation,
            cache_configuration_operation,
            add_library_folder,
            pick_app_folder,
            get_snapshot,
            plan_cache,
            get_cache_telemetry,
            get_cache_runtime,
            get_folder_runtime,
            folder_operation,
            cache_operation
        ])
        .run(tauri::generate_context!())
        .expect("Super HDD could not start");
}
