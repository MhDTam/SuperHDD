export interface Volume {
  letter: string;
  label: string;
  fileSystem: string;
  size: number;
  free: number;
  partitionNumber: number;
  system: boolean;
  health: string;
  encryption: string;
}
export interface Disk {
  number: number;
  id: string;
  model: string;
  serial: string;
  kind: "SSD" | "HDD" | "Unknown";
  mediaEvidence?: string;
  bus: string;
  size: number;
  online: boolean;
  readOnly: boolean;
  system: boolean;
  volumes: Volume[];
}
export interface Snapshot {
  disks: Disk[];
  drivers: { name: string; state: string }[];
  serviceState: string;
  collectedAt: string;
}
export interface CacheRequest {
  sourceDiskId: string;
  sourceLetter: string;
  ssdDiskId: string;
  ssdLetter: string;
  capacityBytes: number;
  scope?: { mode: "selected"; folders: string[] };
}
export interface CachePlan {
  request: CacheRequest;
  reserveBytes: number;
  remainingBytes: number;
  recommendedBytes: number;
  activationAvailable: boolean;
  reasons: string[];
}
export type Page =
  | "home"
  | "library"
  | "acceleration"
  | "performance"
  | "storage"
  | "settings";
export interface Telemetry {
  protocolVersion:number;experimentalAttachEnabled:boolean;
  attached:boolean;paused:boolean;hits:number;misses:number;
  sourceReadBytes:number;ssdReadBytes:number;fillBytes:number;
  evictions:number;errors:number;generation:number;
  capacityBytes:number;sourceLength:number;activeMutations:number;pendingReads:number;
  cacheDisabled:boolean;readBypasses:number;queueLimit:number;maxReadBytes:number;
  nativeVolume?: {
    volumeCount:number;sourceDiskNumber:number|null;backingDiskNumber:number|null;
    pnpState:number;storeLocked:boolean;allocatedBytes:number;cancelledReads:number;
    childCancellations:number;pagingHits:number;revalidatedRecords:number;validationReadBytes:number;
  };
}
export interface RuntimeState {
  backend?:string;
  cachedFiles?:{path:string;residentBytes:number}[];
  phase:string;canStart:boolean;canControl:boolean;canRemove:boolean;reason:string|null;
  nativeVersion:number|null;sourceVolume:string|null;
  request?:CacheRequest|null;
  cacheUsedBytes?:number|null;
}
export type CacheOperation='start'|'stop'|'pause'|'resume'|'clear'|'remove';
export interface CachedFile {path:string;residentBytes:number}
export interface LibraryApp {cacheMode?:'full'|'dataOnly';dataCachePath?:string|null;dataCacheAvailable?:boolean;sharedPool?:boolean;sharedCapacityBytes?:number|null;cachePolicy?:string;id:string;name:string;path:string;origin:string;kind:string;iconDataUri:string|null;managed:boolean;phase:string;eligible:boolean;reason:string|null;cacheUsedBytes:number;allocationBytes:number;canControl:boolean;cachedFiles?:CachedFile[];configurationId?:string|null;sourceLetter?:string|null;ssdLetter?:string|null}
export interface CacheConfiguration {cacheUsedBytes?:number;id:string;name:string;sourceDiskId:string;sourceLetter:string;ssdDiskId:string;ssdLetter:string;budgetBytes:number;sourceVolumeId?:string;ssdVolumeId?:string;allocatedBytes?:number;activeCount?:number}
export interface ConfigurationCommand {operation:'save'|'stop'|'remove';id?:string;configuration?:CacheConfiguration}
export interface AppLibrary {cacheUsedBytes?:number;cachePolicy?:string;apps:LibraryApp[];budgetBytes:number;allocatedBytes:number;available:boolean;legacyActive:boolean;configurations?:CacheConfiguration[]}
export interface LibraryCommand {operation:'start'|'stop'|'pause'|'resume'|'clear'|'remove'|'budget'|'modeData'|'modeFull';id?:string;app?:LibraryApp;budgetBytes?:number}
