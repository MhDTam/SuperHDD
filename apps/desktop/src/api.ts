import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Snapshot, CacheRequest, CachePlan, AppLibrary, LibraryCommand, ConfigurationCommand } from "./types";
import type {Telemetry} from './types';
import type {RuntimeState,CacheOperation} from './types';
export const demo = new URLSearchParams(location.search).get("demo") === "1";
export async function benchmarkStatus():Promise<any>{if(demo||!isTauri())return {phase:'idle',running:false,canRecover:false,report:null};return invoke('benchmark_status')}
export async function benchmarkAction(command:{action:'start'|'cancel'|'recover'|'openReport'|'openFolder';configurationId?:string;profile?:string;workload?:string}):Promise<any>{if(demo||!isTauri())throw new Error('desktopRequired');return invoke('benchmark_action',{command})}
const GiB = 1024 ** 3;
const demoSnapshot: Snapshot = {
  disks: [
    {
      number: 1,
      id: "demo-hdd",
      model: "Seagate BarraCuda",
      serial: "DEMO",
      kind: "HDD",
      bus: "SATA",
      size: 4 * 1024 * GiB,
      online: true,
      readOnly: false,
      system: false,
      volumes: [
        {
          letter: "D:",
          label: "Archive",
          fileSystem: "NTFS",
          size: 4 * 1024 * GiB,
          free: 400 * GiB,
          partitionNumber: 1,
          system: false,
          health: "Healthy",
          encryption: "FullyDecrypted",
        },
      ],
    },
    {
      number: 0,
      id: "demo-ssd",
      model: "Samsung 980 PRO",
      serial: "DEMO",
      kind: "SSD",
      bus: "NVMe",
      size: 1024 * GiB,
      online: true,
      readOnly: false,
      system: true,
      volumes: [
        {
          letter: "C:",
          label: "Windows",
          fileSystem: "NTFS",
          size: 1024 * GiB,
          free: 420 * GiB,
          partitionNumber: 2,
          system: true,
          health: "Healthy",
          encryption: "FullyDecrypted",
        },
      ],
    },
  ],
  drivers: [],
  serviceState: "NotInstalled",
  collectedAt: "Demo",
};
export async function snapshot(force = false): Promise<Snapshot> {
  if (demo) return structuredClone(demoSnapshot);
  if (!isTauri()) throw new Error("desktopRequired");
  return invoke("get_snapshot", { force });
}
export async function plan(request: CacheRequest): Promise<CachePlan> {
  if (demo)
    return {
      request,
      reserveBytes: 100 * GiB,
      remainingBytes: 420 * GiB - request.capacityBytes,
      recommendedBytes: 150 * GiB,
      activationAvailable: false,
      reasons: ["demoOnly", ...(request.scope ? ["selectedScopeUnavailable"] : [])],
    };
  return invoke("plan_cache", { request });
}
export async function telemetry():Promise<Telemetry|null>{
  if(demo||!isTauri())return null;
  try{return await invoke('get_cache_telemetry')}catch{return null}
}
export async function cacheRuntime():Promise<RuntimeState>{
  if(demo||!isTauri())return {phase:'stopped',canStart:false,canControl:false,canRemove:false,reason:demo?'demoOnly':'desktopRequired',nativeVersion:null,sourceVolume:null};
  try {
    const folder = await invoke<RuntimeState>('get_folder_runtime');
    if (folder && (folder.canStart || ['active','paused','starting','faulted'].includes(folder.phase))) return folder;
  } catch { /* Older local hosts still expose the volume backend only. */ }
  return invoke('get_cache_runtime');
}
export async function cacheOperation(operation:CacheOperation,request:CacheRequest|null=null):Promise<RuntimeState>{
  if(demo||!isTauri())throw new Error('desktopRequired');
  const current = await cacheRuntime();
  return invoke(request?.scope || ['dokanFolder','dokanLibrary'].includes(current.backend||'') ? 'folder_operation' : 'cache_operation',{command:{operation,request}});
}
export async function appLibrary(refresh=false):Promise<AppLibrary>{if(demo||!isTauri())return {apps:[],budgetBytes:24_999_952_384,allocatedBytes:0,available:false,legacyActive:false};return invoke('get_app_library',{refresh})}
export async function libraryOperation(command:LibraryCommand):Promise<AppLibrary>{return invoke('app_library_operation',{command})}
export async function addLibraryFolder(path:string,name:string|null):Promise<AppLibrary>{return invoke('add_library_folder',{path,name})}
export async function pickAppFolder():Promise<string|null>{return invoke('pick_app_folder')}
export async function configurationOperation(command:ConfigurationCommand):Promise<AppLibrary>{return invoke('cache_configuration_operation',{command})}
export async function windowAction(
  action: "minimize" | "toggleMaximize" | "close",
) {
  if (!isTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow()[action]();
}
