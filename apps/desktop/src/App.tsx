import { useEffect, useRef, useState, type ReactNode, type HTMLAttributes } from "react";
import {
  Home,
  Gauge,
  ChartNoAxesCombined,
  Gamepad2,
  Database,
  Settings,
  ArrowRight,
  ShieldCheck,
  HardDrive,
  Zap,
  MousePointer2,
  Heart,
  Minus,
  Square,
  X,
  ChevronDown,
  Info,
  RefreshCw,
  Folder,
  AlertCircle,
  Languages,
  Moon,
  Sun,
  Activity,
  Layers,
  Clock,
  MemoryStick,
  Shield,
  CheckCircle2,
  Trash2,
} from "lucide-react";
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import * as api from "./api";
import AppLibrary from './AppLibrary';
import BenchmarkPanel from './BenchmarkPanel';
import {formatBytes} from './format-bytes';
import type {
  Snapshot,
  Disk,
  Volume,
  Page,
  CachePlan,
  Telemetry,
  RuntimeState,
  CacheOperation,
  AppLibrary as LibraryState,
} from "./types";
import en from "./locales/en.json";
import ar from "./locales/ar.json";
import { loadFolders, normalizeFolder } from "./cache-selection";
import { appendHistory, demoHistory, difference, emptyHistory, historyKey, loadHistory, selectHistory, serializeHistory, windows, type Period } from './performance-history';
type Key = keyof typeof en;
const GiB = 1024 ** 3;
const GB = 1_000_000_000;
const cacheBlockBytes = 64 * 1024;
const minCacheGB = 1.1;
const maxCacheGB = 25;
const cacheBudgetBytes = (gb: number) =>
  Math.floor((gb * GB) / cacheBlockBytes) * cacheBlockBytes;
const nav: { id: Page; icon: typeof Home }[] = [
  { id: "home", icon: Home },
  { id: "library", icon: Gamepad2 },
  { id: "acceleration", icon: Gauge },
  { id: "performance", icon: ChartNoAxesCombined },
  { id: "storage", icon: Database },
  { id: "settings", icon: Settings },
];
function Card({
  children,
  className = "",
  ...attributes
}: {
  children: ReactNode;
  className?: string;
} & HTMLAttributes<HTMLElement>) {
  return <section {...attributes} className={`glass ${className}`}>{children}</section>;
}
function Toggle({
  checked,
  onChange,
  disabled = false,
  label,
}: {
  checked: boolean;
  onChange?: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={label}
      aria-checked={checked}
      disabled={disabled}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={onChange}
    >
      <span />
    </button>
  );
}
function Hardware({
  kind = "HDD",
  bus,
  small = false,
}: {
  kind?: string;
  bus?: string;
  small?: boolean;
}) {
  if (kind === "SSD" && bus?.toLowerCase() === "nvme") {
    return (
      <img
        className={`hardware nvme ${small ? "small" : ""}`}
        src="/assets/nvme-drive.png"
        alt=""
        aria-hidden="true"
      />
    );
  }
  return (
    <div
      className={`hardware ${kind === "SSD" ? "ssd" : "hdd"} ${small ? "small" : ""}`}
      aria-hidden="true"
    >
      {kind === "SSD" ? (
        <>
          <span className="ssd-line" />
          <span className="ssd-mark">SSD</span>
          <span className="ssd-contacts" />
        </>
      ) : (
        <>
          <div className="platter">
            <span />
          </div>
          <div className="arm" />
          <i />
          <i />
          <i />
          <i />
        </>
      )}
    </div>
  );
}
export default function App() {
  const [language, setLanguage] = useState<"en" | "ar">(() =>
    localStorage.getItem("superhdd-language") === "ar" ? "ar" : "en",
  );
  const [theme, setTheme] = useState(
    () => localStorage.getItem("superhdd-theme") || "light",
  );
  const [reduced, setReduced] = useState(
    () => localStorage.getItem("superhdd-motion") === "reduced",
  );
  const [welcome, setWelcome] = useState(
    () => !localStorage.getItem("superhdd-welcome"),
  );
  const [page, setPage] = useState<Page>("home");
  const [telemetry,setTelemetry]=useState<Telemetry|null>(null);
  const [runtime,setRuntime]=useState<RuntimeState|null>(null);
  const [libraryReady,setLibraryReady]=useState(false);
  const [homeLibrary,setHomeLibrary]=useState<LibraryState|null>(null);
  const [homeConfiguration,setHomeConfiguration]=useState("");
  useEffect(()=>{
    let alive=true;
    const read=()=>{if(document.hidden)return;void api.appLibrary().then(value=>{
      if(alive){setLibraryReady(value.available);setHomeLibrary(value)}
    }).catch(()=>{})};
    read();const timer=page==='home'?setInterval(read,4000):undefined;
    return()=>{alive=false;clearInterval(timer)};
  },[page]);
  useEffect(()=>{
    let alive=true;
    const read=()=>{if(document.hidden)return;void api.cacheRuntime().then(s=>{if(alive)setRuntime(s)}).catch(()=>{if(alive)setRuntime(null)})};
    read();const timer=setInterval(read,3000);return()=>{alive=false;clearInterval(timer)};
  },[]);
  const [historyNow, setHistoryNow] = useState(Date.now);
  const [performanceHistory, setPerformanceHistory] = useState(() => {
    if (api.demo) return demoHistory(Date.now());
    try { return loadHistory(localStorage.getItem(historyKey), Date.now()); } catch { return emptyHistory(); }
  });
  const recordedHistory = useRef(performanceHistory);
  const [historyStorageFailed, setHistoryStorageFailed] = useState(false);
  const [period, setPeriod] = useState<Period>('live');
  const latestLive = selectHistory(performanceHistory, 'live', historyNow).points.at(-1);
  const history = latestLive && latestLive.at >= historyNow - 10_000 && telemetry?.attached && !telemetry.paused && !telemetry.cacheDisabled
    ? [{ssd: latestLive.ssd ?? 0, hdd: latestLive.hdd ?? 0}] : [];
  useEffect(()=>{
    if(api.demo)return;
    let alive=true, collecting=false;let previous:{at:number;stats:Telemetry}|null=null;
    let lastSaved = Date.now();
    const save = () => {
      try { localStorage.setItem(historyKey, serializeHistory(recordedHistory.current)); if(alive)setHistoryStorageFailed(false); }
      catch { if(alive)setHistoryStorageFailed(true); }
      lastSaved = Date.now();
    };
    const collect=async()=>{
      if(collecting)return;
      setHistoryNow(Date.now());
      if(document.hidden){previous=null;return}
      collecting=true;
      try {
        const stats=await api.telemetry();if(!alive)return;
        const now=Date.now();setHistoryNow(now);setTelemetry(stats);
        if(stats && previous){
          const sample=difference(previous.stats,stats,previous.at,now);
          if(sample){
            recordedHistory.current=appendHistory(recordedHistory.current,sample);
            setPerformanceHistory(recordedHistory.current);
          }
        }
        previous=stats?{at:now,stats}:null;
        if(now-lastSaved>=15_000)save();
      } finally { collecting=false; }
    };
    const visibility=()=>{previous=null;if(document.hidden)save();else void collect()};
    document.addEventListener('visibilitychange',visibility);window.addEventListener('pagehide',save);
    void collect();const timer=setInterval(collect,1500);
    return()=>{alive=false;clearInterval(timer);document.removeEventListener('visibilitychange',visibility);window.removeEventListener('pagehide',save);save()};
  },[]);
  const [data, setData] = useState<Snapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [source, setSource] = useState(() => localStorage.getItem("superhdd-source") || "");
  const [ssd, setSsd] = useState(() => localStorage.getItem("superhdd-ssd") || "");
  const [capacity, setCapacity] = useState(() => {
    const saved = localStorage.getItem("superhdd-capacity-gb");
    const value = Number(saved);
    return saved && Number.isFinite(value) && value >= minCacheGB && value <= maxCacheGB
      ? saved
      : "25";
  });
  const [sizeMode, setSizeMode] = useState("custom");
  const [scopeMode, setScopeMode] = useState(() => {
    if (!localStorage.getItem("superhdd-folder-workflow-v1")) {
      localStorage.setItem("superhdd-folder-workflow-v1", "1");
      localStorage.setItem("superhdd-scope", "selected");
    }
    return localStorage.getItem("superhdd-scope") === "volume" ? "volume" : "selected";
  });
  const [folderPreferences, setFolderPreferences] = useState(loadFolders);
  const [folderInput, setFolderInput] = useState("");
  const [folderError, setFolderError] = useState("");
  const importedFolderSession = useRef("");
  useEffect(() => {
    const request = runtime?.request;
    if (runtime?.backend !== 'dokanFolder' || !request?.scope || !['active','paused','faulted'].includes(runtime.phase)) return;
    const binding = JSON.stringify(request);
    if (importedFolderSession.current === binding) return;
    importedFolderSession.current = binding;
    const key = `${request.sourceDiskId}|${request.sourceLetter}`;
    setSource(key); setSsd(`${request.ssdDiskId}|${request.ssdLetter}`); setScopeMode('selected');
    setCapacity(String(Math.round(request.capacityBytes / GB * 1000) / 1000));
    localStorage.setItem('superhdd-source', key);
    localStorage.setItem('superhdd-ssd', `${request.ssdDiskId}|${request.ssdLetter}`);
    localStorage.setItem('superhdd-scope', 'selected');
    setFolderPreferences(previous => {
      const next = {...previous, [key]: request.scope!.folders};
      localStorage.setItem('superhdd-selected-folders', JSON.stringify(next));
      return next;
    });
  }, [runtime]);
  const [plan, setPlan] = useState<CachePlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [modal, setModal] = useState<"plan" | null>(
    null,
  );
  const [diagnosticsOpen,setDiagnosticsOpen]=useState(false);
  const [settingsTab, setSettingsTab] = useState<Key>("general");
  useEffect(() => {
    if (!modal) return;
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
    if (!dialog) return;
    const previous = document.activeElement as HTMLElement | null;
    const background = Array.from(document.querySelectorAll<HTMLElement>('.sidebar, .window-controls, .window-drag, main'));
    const attributes = background.map(element => ({ element, inert: element.getAttribute('inert'), hidden: element.getAttribute('aria-hidden') }));
    for (const { element } of attributes) { element.setAttribute('inert', ''); element.setAttribute('aria-hidden', 'true'); }
    dialog.focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); setModal(null); return; }
      if (event.key !== 'Tab') return;
      const controls = Array.from(dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), a[href], [tabindex="0"]')).filter(element => element.getClientRects().length > 0);
      const first = controls[0], last = controls[controls.length - 1];
      if (!first) { event.preventDefault(); dialog.focus(); return; }
      if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog)) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === dialog)) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', keydown, true);
    return () => {
      document.removeEventListener('keydown', keydown, true);
      for (const { element, inert, hidden } of attributes) {
        if (inert === null) element.removeAttribute('inert'); else element.setAttribute('inert', inert);
        if (hidden === null) element.removeAttribute('aria-hidden'); else element.setAttribute('aria-hidden', hidden);
      }
      if (previous?.isConnected) previous.focus();
    };
  }, [modal]);
  const t = (key: string) =>
    ((language === "ar" ? ar : en) as Record<string, string>)[key] || key;
  useEffect(() => {
    document.documentElement.lang = language;
    document.documentElement.dir = language === "ar" ? "rtl" : "ltr";
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.motion = reduced ? "reduced" : "full";
    localStorage.setItem("superhdd-language", language);
    localStorage.setItem("superhdd-theme", theme);
    localStorage.setItem("superhdd-motion", reduced ? "reduced" : "full");
  }, [language, theme, reduced]);
  async function refresh(force = false) {
    setLoading(true);
    setError("");
    try {
      setData(await api.snapshot(force));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void refresh();
  }, []);
  const options = (kind: string) =>
    data?.disks
      .filter((d) => d.kind === kind && d.online && !d.readOnly)
      .flatMap((d) =>
        d.volumes
          .filter(
            (v) => v.fileSystem === "NTFS" && (kind !== "HDD" || !v.system),
          )
          .map((v) => ({ disk: d, volume: v, key: `${d.id}|${v.letter}` })),
      ) || [];
  const hdds = options("HDD"),
    ssds = options("SSD");
  const sourceOption = hdds.find((o) => o.key === source) || hdds.find(o => o.volume.letter === "D:") || hdds[0];
  const ssdOption = ssds.find((o) => o.key === ssd) || ssds.find(o => o.volume.letter === "M:") || ssds[0];
  const ssdVolume = ssdOption?.volume;
  const sourceKey = sourceOption?.key || "";
  const selectedFolders = (folderPreferences[sourceKey] || []).filter(path =>
    normalizeFolder(path, sourceOption?.volume.letter || "") === path);
  const validScope = scopeMode === "volume" || selectedFolders.length > 0;
  function saveFolders(paths: string[]) {
    const next = { ...folderPreferences, [sourceKey]: paths };
    localStorage.setItem("superhdd-selected-folders", JSON.stringify(next));
    setFolderPreferences(next);
  }
  function addFolder() {
    const path = normalizeFolder(folderInput, sourceOption?.volume.letter || "");
    if (!path) { setFolderError("folderInvalid"); return; }
    if (selectedFolders.some(item => item.toLowerCase() === path.toLowerCase())) {
      setFolderError("folderDuplicate"); return;
    }
    if (selectedFolders.length >= 64) { setFolderError("folderLimit"); return; }
    saveFolders([...selectedFolders, path]);
    setFolderInput(""); setFolderError("");
  }
  const reserve = ssdVolume ? Math.max(ssdVolume.size * 0.1, 10 * GiB) : 0;
  const maxCapacity = Math.max(
    0,
    Math.min(maxCacheGB, Math.floor((((ssdVolume?.free || 0) - reserve) / GB) * 10) / 10),
  );
  const recommended = Math.min(
    maxCapacity,
    Math.max(minCacheGB, Math.floor((sourceOption?.disk.size || 0) / GB / 10)),
  );
  const chosen = sizeMode === "automatic" ? recommended : Number(capacity);
  const validCapacity = Number.isFinite(chosen) && chosen >= minCacheGB && chosen <= maxCapacity;
  const chosenBytes = validCapacity ? cacheBudgetBytes(chosen) : 0;
  const updateCapacity = (value: string) => {
    setCapacity(value);
    const number = Number(value);
    if (Number.isFinite(number) && number >= minCacheGB && number <= maxCacheGB) {
      localStorage.setItem("superhdd-capacity-gb", value);
    }
  };
  const allVolumes = data?.disks.flatMap((d) => d.volumes) || [];
  const total = allVolumes.reduce((s, v) => s + v.size, 0);
  const free = allVolumes.reduce((s, v) => s + v.free, 0);
  const start = () => {
    setWelcome(false);
    setPage("acceleration");
    localStorage.setItem("superhdd-welcome", "1");
  };
  const go = (value: Page) => {
    setWelcome(false);
    setPage(value);
    setError("");
  };
  async function prepare() {
    if (!sourceOption || !ssdOption || !validCapacity || !validScope) return;
    setBusy(true);
    setError("");
    try {
      setPlan(
        await api.plan({
          sourceDiskId: sourceOption.disk.id,
          sourceLetter: sourceOption.volume.letter,
          ssdDiskId: ssdOption.disk.id,
          ssdLetter: ssdOption.volume.letter,
          capacityBytes: chosenBytes,
          ...(scopeMode === "selected" ? { scope: { mode: "selected" as const, folders: selectedFolders } } : {}),
        }),
      );
      setModal("plan");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function operateCache(operation:CacheOperation){
    if(operation==='start'&&plan?.request.scope&&(!plan.activationAvailable||!['dokanFolder','dokanLibrary'].includes(runtime?.backend||''))){setError(plan.reasons[0]||'selectedScopeUnavailable');return;}
    if(busy)return;setBusy(true);setError('');
    try{setRuntime(await api.cacheOperation(operation,operation==='start'?plan?.request||null:null));setTelemetry(await api.telemetry());setModal(null);if(['start','stop','remove'].includes(operation))setData(await api.snapshot(true))}
    catch(e){setError(String(e))}finally{setBusy(false)}
  }
  const header = (title: Key, description: Key, action?: ReactNode) => (
    <header className="page-header">
      <div>
        <h1>{t(title)}</h1>
        <p>{t(description)}</p>
      </div>
      {action}
    </header>
  );
  const empty = (
    icon: ReactNode,
    title: Key,
    text: Key,
    action?: ReactNode,
  ) => (
    <div className="empty-state">
      <div className="empty-icon">{icon}</div>
      <h3>{t(title)}</h3>
      <p>{t(text)}</p>
      {action}
    </div>
  );
  const driverActive =
    data?.drivers.some(
      (d) => ["SuperHDDCacheDriver", "SuperHDDRawLab"].includes(d.name) && d.state === "Running",
    ) || false;
  const cacheStateKey: Key = !telemetry?.attached ? "noActiveCache"
    : telemetry.cacheDisabled ? "cacheBypass" : telemetry.paused ? "cachePaused" : ['dokanFolder','dokanLibrary'].includes(runtime?.backend||'') ? 'folderCacheActive' : "labCache";
  function DriveCard({
    option,
    kind,
  }: {
    option?: { disk: Disk; volume: Volume };
    kind: "HDD" | "SSD";
  }) {
    return (
      <div className="drive-summary">
        <Hardware kind={kind} bus={option?.disk.bus} />
        <div>
          <h3>
            <bdi>{option?.volume.letter || "—"}</bdi>{" "}
            {t(kind === "HDD" ? "hdd" : "ssd")}
          </h3>
          <p>
            <bdi>
              {option?.disk.model || t(kind === "HDD" ? "noHdd" : "noSsd")}
            </bdi>
          </p>
          <div className="chips">
            <span>{kind}</span>
            <span>
              <bdi>{option?.disk.bus || "—"}</bdi>
            </span>
          </div>
        </div>
      </div>
    );
  }
  function Metric({
    icon,
    label,
    value,
    tint = "blue",
    caption,
  }: {
    icon: ReactNode;
    label: Key;
    value?: string;
    tint?: string;
    caption?: string;
  }) {
    return (
      <Card className="metric">
        <div className={`icon-orb ${tint}`}>{icon}</div>
        <strong>
          <bdi>{value || "—"}</bdi>
        </strong>
        <span>{t(label)}</span>
        <small>{api.demo ? t("sample") : caption || t(value?'live':"notAvailable")}</small>
      </Card>
    );
  }
  function Setup() {
    if(libraryReady||runtime?.backend==='dokanLibrary')return <AppLibrary language={language} settingsOnly snapshot={data} onLibrary={()=>go('library')} onRefresh={()=>void refresh(true)}/>;
    if((runtime?.nativeVersion===6||runtime?.nativeVersion===7||runtime?.nativeVersion===8)&&runtime.canControl&&runtime.request){
      const request=runtime.request;
      return <>
        {header("setupTitle","setupDescription")}
        <Card className="runtime-session">
          <div className="section-heading"><h2>{t("runtimeSession")}</h2><strong role="status">{t(`runtimePhase_${runtime.phase}`)}</strong></div>
          <dl className="cache-facts">
            <div><dt>{t("sourceHdd")}</dt><dd><bdi>{request.sourceLetter}</bdi></dd></div>
            <div><dt>{t("cacheSsd")}</dt><dd><bdi>{request.ssdLetter}</bdi></dd></div>
            <div><dt>{t("cacheSize")}</dt><dd><bdi>{(request.capacityBytes/GB).toLocaleString(language,{maximumFractionDigits:1})} GB</bdi></dd></div>
            <div><dt>{t("cacheUsed")}</dt><dd><bdi>{runtime.cacheUsedBytes==null?"—":formatBytes(runtime.cacheUsedBytes)}</bdi></dd></div>
          </dl>
          {runtime.reason&&<p>{t(runtime.reason)}</p>}
          <div className="runtime-controls">
            {runtime.phase!=="faulted"&&<>
              <button className="secondary" disabled={busy} onClick={()=>void operateCache(runtime.phase==="paused"?"resume":"pause")}>{t(runtime.phase==="paused"?"runtimeResume":"runtimePause")}</button>
              <button className="secondary" disabled={busy} onClick={()=>void operateCache("clear")}>{t("runtimeClear")}</button>
            </>}
            <button className="secondary" disabled={busy} onClick={()=>void operateCache("stop")}>{t("runtimeStop")}</button>
          </div>
        </Card>
        <Card className="cache-contents" data-testid="cache-contents">
          <h2>{t("cacheContents")}</h2><p>{t("nativeFileListUnavailable")}</p><small>{t("clearCopiesHint")}</small>
        </Card>
      </>;
    }
    return (
      <>
        {header("setupTitle", "setupDescription")}

        <div className="two-grid">
          <Card className="drive-selector">
            <h2><span className="setup-step">1</span>{t("sourceHdd")}</h2>
            <div className="select-wrap">
              <select
                aria-label={t("sourceHdd")}
                value={sourceOption?.key || ""}
                onChange={(e) => { setSource(e.target.value); setFolderInput(""); setFolderError(""); localStorage.setItem("superhdd-source", e.target.value); }}
                disabled={!hdds.length}
              >
                {!hdds.length && <option value="">{t("noHdd")}</option>}
                {hdds.map((o) => (
                  <option key={o.key} value={o.key}>
                    {o.volume.letter} {o.disk.model}
                  </option>
                ))}
              </select>
              <ChevronDown size={18} />
            </div>
            <DriveCard option={sourceOption} kind="HDD" />
            {sourceOption && (
              <>
                <div className="capacity-note">
                  <span>
                    <bdi>
                      {formatBytes(
                        sourceOption.volume.size - sourceOption.volume.free,
                      )}
                    </bdi>{" "}
                    {t("used")}
                  </span>
                  <bdi>
                    {Math.round(
                      (1 -
                        sourceOption.volume.free / sourceOption.volume.size) *
                        100,
                    )}
                    %
                  </bdi>
                </div>
                <div className="progress">
                  <span
                    style={{
                      width: `${(1 - sourceOption.volume.free / sourceOption.volume.size) * 100}%`,
                    }}
                  />
                </div>
              </>
            )}
          </Card>
          <Card className="drive-selector">
            <h2><span className="setup-step">2</span>{t("cacheSsd")}</h2>
            <div className="select-wrap">
              <select
                aria-label={t("cacheSsd")}
                value={ssdOption?.key || ""}
                onChange={(e) => { setSsd(e.target.value); localStorage.setItem("superhdd-ssd", e.target.value); }}
                disabled={!ssds.length}
              >
                {!ssds.length && <option value="">{t("noSsd")}</option>}
                {ssds.map((o) => (
                  <option key={o.key} value={o.key}>
                    {o.volume.letter} {o.disk.model}
                  </option>
                ))}
              </select>
              <ChevronDown size={18} />
            </div>
            <DriveCard option={ssdOption} kind="SSD" />
            {ssdVolume && (
              <>
                <div className="capacity-note">
                  <span>
                    <bdi>{formatBytes(ssdVolume.free)}</bdi> {t("free")}
                  </span>
                  <bdi>
                    {Math.round((ssdVolume.free / ssdVolume.size) * 100)}%
                  </bdi>
                </div>
                <div className="progress powder">
                  <span
                    style={{
                      width: `${(ssdVolume.free / ssdVolume.size) * 100}%`,
                    }}
                  />
                </div>
              </>
            )}
          </Card>
        </div>
        <Card className="allocation">
          <div className="section-heading">
            <h2>
              <span className="setup-step">3</span>{t("cacheSize")} <Info size={17} />
            </h2>
            <label className="number-input">
              <input
                aria-label={t("cacheSize")}
                type="number"
                min={minCacheGB}
                max={maxCapacity || minCacheGB}
                step="0.1"
                value={sizeMode === "automatic" ? String(chosen) : capacity}
                disabled={!ssdVolume || sizeMode === "automatic"}
                onChange={(e) => updateCapacity(e.target.value)}
              />
              <bdi>GB</bdi>
            </label>
          </div>
          <input
            className="range"
            aria-label={`${t("cacheSize")} slider`}
            type="range"
            min={minCacheGB}
            max={Math.max(minCacheGB, maxCapacity)}
            step="0.1"
            value={Number.isFinite(chosen) ? Math.max(minCacheGB, Math.min(chosen, maxCapacity)) : minCacheGB}
            disabled={!ssdVolume || sizeMode === "automatic"}
            onChange={(e) => updateCapacity(e.target.value)}
          />
          <div className="range-labels">
            <bdi>{minCacheGB} GB</bdi>
            <bdi>{maxCapacity} GB</bdi>
          </div>
          <div className="two-grid mode-options">
            {["automatic", "custom"].map((mode) => (
              <button
                key={mode}
                className={`mode-option ${sizeMode === mode ? "selected" : ""}`}
                aria-pressed={sizeMode === mode}
                onClick={() => setSizeMode(mode)}
              >
                <span className="radio">{sizeMode === mode && <span />}</span>
                <span>
                  <strong>{t(mode)}</strong>
                  <small>
                    {t(mode === "automatic" ? "automaticHint" : "customHint")}
                  </small>
                </span>
              </button>
            ))}
          </div>
          <div className="two-grid allocation-hints">
            <div>
              <div className="icon-orb green">
                <ShieldCheck />
              </div>
              <span>
                <strong>
                  {t("recommended")}: <bdi>{recommended || "—"} GB</bdi>
                </strong>
                <small>
                  {t("reserve")}: <bdi>{formatBytes(reserve)}</bdi>
                </small>
              </span>
            </div>
            <div>
              <div className="icon-orb">
                <HardDrive />
              </div>
              <span>
                <strong>
                  {t("remaining")}:{" "}
                  <bdi>
                    {ssdVolume
                      ? formatBytes(Math.max(0, ssdVolume.free - chosenBytes))
                      : "—"}
                  </bdi>
                </strong>
                <small>{t("customHint")}</small>
              </span>
            </div>
          </div>
        </Card>
        <Card className="cache-scope">
          <h2><span className="setup-step">4</span>{t("cacheScope")}</h2>
          <p>{t("cacheScopeHint")}</p>
          <div className="two-grid mode-options">
            {["volume", "selected"].map(mode => (
              <button key={mode} className={`mode-option ${scopeMode === mode ? "selected" : ""}`}
                aria-pressed={scopeMode === mode}
                onClick={() => { setScopeMode(mode); localStorage.setItem("superhdd-scope", mode); }}>
                <span className="radio">{scopeMode === mode && <span />}</span>
                <span><strong>{t(mode === "selected" ? "selectedOnly" : "wholeVolume")}</strong>
                  <small>{t(mode === "selected" ? "selectedOnlyHint" : "wholeVolumeHint")}</small></span>
              </button>
            ))}
          </div>
          {scopeMode === "selected" && <>
            <form className="folder-entry" onSubmit={event => { event.preventDefault(); addFolder(); }}>
              <label className="form-label">{t("appFolder")}
                <input className="input" dir="ltr" value={folderInput} disabled={!sourceOption}
                  placeholder={`${sourceOption?.volume.letter || "D:"}\\Games\\My Game`}
                  onChange={event => { setFolderInput(event.target.value); setFolderError(""); }} />
              </label>
              <button className="secondary" type="submit" disabled={!sourceOption || !folderInput.trim()}><Folder size={18}/>{t("addFolder")}</button>
            </form>
            {folderError && <p className="error-text" role="alert">{t(folderError)}</p>}
            {selectedFolders.length ? <ul className="selected-folders" aria-label={t("selectedFolders")}>
              {selectedFolders.map(path => <li key={path}>
                <Folder size={20}/><span><strong><bdi>{path.split("\\").at(-1)}</bdi></strong><bdi dir="ltr">{path}</bdi></span>
                <button className="icon-button" aria-label={`${t("removeFolder")} ${path}`}
                  onClick={() => saveFolders(selectedFolders.filter(item => item !== path))}><X size={18}/></button>
              </li>)}
            </ul> : <p>{t("noSelectedFolders")}</p>}
            <p className="scope-notice"><Info size={18}/>{t(['dokanFolder','dokanLibrary'].includes(runtime?.backend||'') ? "folderScopeHint" : "selectedScopeUnavailable")}</p>
            <small>{t(['dokanFolder','dokanLibrary'].includes(runtime?.backend||'') ? "folderChooseOne" : "selectionDraftHint")}</small>
          </>}
          <button className="primary wide" disabled={busy || !sourceOption || !ssdOption || !validCapacity || !validScope}
            onClick={() => void prepare()}>
            {busy ? (
              <RefreshCw className="spin" size={20} />
            ) : (
              <>
                {t("enable")}
                <ArrowRight size={21} />
              </>
            )}
          </button>
        </Card>
        <Card className="cache-contents" data-testid="cache-contents">
          <div className="section-heading"><h2><span className="setup-step">5</span>{t("cacheContents")}</h2>
            <button className="secondary" disabled={busy || !runtime?.canControl || runtime.phase === "faulted"}
              onClick={() => void operateCache("clear")}><Trash2 size={18}/>{t("runtimeClear")}</button>
          </div>
          <dl className="cache-facts">
            <div><dt>{t("cacheSize")}</dt><dd><bdi>{telemetry?.attached ? formatBytes(telemetry.capacityBytes) : "—"}</bdi></dd></div>
            <div><dt>{t("cacheUsed")}</dt><dd>{['dokanFolder','dokanLibrary'].includes(runtime?.backend||'') && runtime?.cacheUsedBytes != null ? <bdi>{formatBytes(runtime.cacheUsedBytes)}</bdi> : t("notAvailable")}</dd></div>
          </dl>
          {['dokanFolder','dokanLibrary'].includes(runtime?.backend||'') ? (runtime?.cachedFiles?.length ? <ul className="selected-folders">
            {runtime.cachedFiles.map(file => <li key={file.path}><Folder size={20}/><span><bdi dir="ltr">{file.path}</bdi><small><bdi>{formatBytes(file.residentBytes)}</bdi></small></span></li>)}
          </ul> : <p>{t("folderCacheEmpty")}</p>) : <p>{t(telemetry?.attached || runtime?.phase === "active" || runtime?.phase === "paused" ? "cacheFileListUnavailable" : runtime?.phase === "stopped" ? "cacheEmpty" : "cacheStateUnavailable")}</p>}
          <small>{t("clearCopiesHint")}</small>
        </Card>
        <Card className="runtime-session">
          <div className="section-heading"><h2>{t('runtimeSession')}</h2><strong role="status">{runtime?t(`runtimePhase_${runtime.phase}`):t('runtimeUnavailable')}</strong></div>
          {runtime?.reason&&<p>{t(runtime.reason)}</p>}
          {runtime?.canControl&&<div className="runtime-controls">
            {runtime.phase!=='faulted'&&<button className="secondary" disabled={busy} onClick={()=>void operateCache(runtime.phase==='paused'?'resume':'pause')}>{t(runtime.phase==='paused'?'runtimeResume':'runtimePause')}</button>}
            <button className="secondary" disabled={busy} onClick={()=>void operateCache('stop')}>{t('runtimeStop')}</button>
          </div>}
          {runtime?.canRemove&&runtime.phase==='stopped'&&<button className="secondary" disabled={busy} onClick={()=>void operateCache('remove')}>{t('runtimeRemove')}</button>}
        </Card>
      </>
    );
  }
  function Dashboard() {
    const configurations=homeLibrary?.configurations||[];
    const configuration=configurations.find(c=>c.id===homeConfiguration)
      ||configurations.find(c=>(c.activeCount||0)>0)||configurations[0];
    const findDrive=(id?:string,letter?:string)=>{
      const disk=data?.disks.find(d=>d.id===id);
      const volume=disk?.volumes.find(v=>v.letter===letter);
      return disk&&volume?{disk,volume}:undefined;
    };
    const sourceDrive=libraryReady?findDrive(configuration?.sourceDiskId,configuration?.sourceLetter):sourceOption;
    const cacheDrive=libraryReady?findDrive(configuration?.ssdDiskId,configuration?.ssdLetter):ssdOption;
    const activeApps=homeLibrary?.apps.filter(app=>app.managed&&app.phase==='active')||[];
    const pausedApps=homeLibrary?.apps.filter(app=>app.managed&&app.phase==='paused')||[];
    const status:Key=libraryReady
      ?activeApps.length?'folderCacheActive':pausedApps.length?'cachePaused':'noActiveCache'
      :cacheStateKey;
    const cacheUsed=libraryReady&&homeLibrary
      ?formatBytes(homeLibrary.cacheUsedBytes??homeLibrary.apps.reduce((sum,app)=>sum+app.cacheUsedBytes,0))
      :['dokanFolder','dokanLibrary'].includes(runtime?.backend||'')&&runtime?.cacheUsedBytes!=null
        ?formatBytes(runtime.cacheUsedBytes):undefined;
    return (
      <>
        {header(
          "setupTitle",
          libraryReady?"homeDescription":"setupDescription",
          <div className="status-pill">
            <span className="status-dot" />
            {t(status)}
          </div>,
        )}
        {libraryReady&&configuration&&<div className="home-configuration">
          <label htmlFor="home-configuration">{t("homeConfiguration")}</label>
          <select id="home-configuration" className="input" value={configuration.id} onChange={event=>setHomeConfiguration(event.target.value)}>
            {configurations.map(c=><option key={c.id} value={c.id}>{'\u2068'}{c.name}{'\u2069'} · {'\u2066'}{c.sourceLetter} → {c.ssdLetter}{'\u2069'}</option>)}
          </select>
          <span><bdi>{formatBytes(configuration.budgetBytes)}</bdi> {t("cacheSize")}</span>
        </div>}
        <Card className="dashboard-hero">
          <div className="hero-device source">
            <h2>
              <bdi>{sourceDrive?.volume.letter || configuration?.sourceLetter || "—"}</bdi> {t("hdd")}
            </h2>
            <p>
              <bdi>{sourceDrive?.disk.model || t("noHdd")}</bdi>
            </p>
            <div className="chips">
              <span>HDD</span>
              <span>
                <bdi>{sourceDrive?.disk.bus || "—"}</bdi>
              </span>
            </div>
          </div>
          <img className="storage-art" src="/assets/storage-hero.png" alt="" />
          <div className="hero-device destination">
            <h2>
              <bdi>{cacheDrive?.volume.letter || configuration?.ssdLetter || "—"}</bdi> {t("ssd")}
            </h2>
            <p>
              <bdi>{cacheDrive?.disk.model || t("noSsd")}</bdi>
            </p>
            <div className="chips">
              <span>SSD</span>
              <span>
                <bdi>{cacheDrive?.disk.bus || "—"}</bdi>
              </span>
            </div>
          </div>
          <div className="hero-center-note">
            <ShieldCheck size={18} />
            {libraryReady?(configuration?<bdi dir="ltr">{configuration.sourceLetter} → {configuration.ssdLetter}</bdi>:t("homeNoConfiguration")):t(status)}
          </div>
        </Card>
        {libraryReady&&<p className="home-metrics-scope">{t("homeAllConfigurations")}</p>}
        {(libraryReady || api.demo || telemetry?.attached) && <div className="four-grid">
          <Metric icon={<Gauge />} label="cacheHitRate" value={telemetry&&telemetry.hits+telemetry.misses?`${(100*telemetry.hits/(telemetry.hits+telemetry.misses)).toFixed(1)}%`:undefined}/>
          <Metric icon={<Database />} label="cacheUsed" value={cacheUsed}/>
          <Metric icon={<ArrowRight />} label="ssdReads" tint="lavender" value={history.length?`${history[history.length-1].ssd.toFixed(2)} GiB/s`:undefined}/>
          <Metric icon={<Activity />} label="hddReads" tint="pink" value={history.length?`${history[history.length-1].hdd.toFixed(2)} GiB/s`:undefined}/>
        </div>}
        <Card className="notice-bar">
          <Info size={20} />
          <span>{t(libraryReady?"homeCacheHint":telemetry?.attached ? "labCacheHint" : "driverUnavailableText")}</span>
          <button className="text-button" onClick={() => go("acceleration")}>
            {t("configure")}
            <ArrowRight size={17} />
          </button>
        </Card>
        <div className="dashboard-actions">
          <button className="primary" onClick={() => go(libraryReady?"library":"acceleration")}>
            {libraryReady?<Gamepad2 size={20}/>:<Gauge size={20} />}
            {t(libraryReady?"homeOpenLibrary":"configure")}
          </button>
          <button className="secondary" onClick={() => go("storage")}>
            <Database size={20} />
            {t("storage")}
          </button>
          <button className="secondary" onClick={() => go("performance")}>
            <ChartNoAxesCombined size={20} />
            {t("performance")}
          </button>
        </div>
      </>
    );
  }
  function PerformanceCharts() {
    const selected = selectHistory(performanceHistory, period, historyNow);
    const total = selected.total;
    const hasSamples = selected.samples.length > 0;
    const active = api.demo || (telemetry?.attached && !telemetry.paused && !telemetry.cacheDisabled);
    const hit = total.hits + total.misses ? 100 * total.hits / (total.hits + total.misses) : null;
    const throughput = total.seconds ? (total.ssdBytes + total.sourceBytes) / total.seconds / GiB : null;
    const dateLabel = (at: number, detail = false) => new Date(at).toLocaleString(language, period === 'live' || period === 'hour'
      ? {hour:'2-digit',minute:'2-digit',...(detail || period === 'live' ? {second:'2-digit'} : {})}
      : {month:'short',day:'numeric',...(detail || period === 'day' ? {hour:'2-digit',minute:'2-digit'} : {})});
    const duration = (seconds: number) => {
      const unit = seconds >= 3600 ? 'hour' : seconds >= 60 ? 'minute' : 'second';
      return new Intl.NumberFormat(language, {style:'unit',unit,unitDisplay:'long',maximumFractionDigits:1}).format(seconds / (unit === 'hour' ? 3600 : unit === 'minute' ? 60 : 1));
    };
    return (
      <>
        {header(
          "performanceTitle",
          "performanceDescription",
          <div className="segmented" role="group" aria-label={t('performancePeriod')}>
            {(["live", "hour", "day", "week", "month"] as Period[]).map((p) => (
              <button
                key={p}
                aria-pressed={period === p}
                className={period === p ? "active" : ""}
                onClick={() => setPeriod(p)}
              >
                {t(p)}
              </button>
            ))}
          </div>,
        )}
        <BenchmarkPanel language={language}/>
        <div className="performance-history-note" role="status" data-testid="performance-window">
          <div><strong>{t(period === 'live' ? 'performanceLiveWindow' : 'performanceSelectedWindow')}: {t(period)}</strong>
            <span><bdi>{dateLabel(selected.start, true)} — {dateLabel(selected.end, true)}</bdi></span></div>
          <p>{api.demo ? t('sample') : t('performanceHistoryHint')}</p>
          <p>{hasSamples ? <>{t('performanceRecorded')}: <bdi>{duration(total.seconds)}</bdi> · {t('performanceAverageHint')}</> : t('performanceNoHistory')}</p>
          {historyStorageFailed && <p role="alert">{t('performanceStorageFailed')}</p>}
        </div>
        {!active && <Card className="benchmark-notice"><Info size={20}/><p>{t('performanceRecordingStopped')}</p></Card>}
        <div className="two-grid charts">
          {(["cacheHitRate", "readThroughput"] as Key[]).map((label, index) => (
            <Card key={label} className="chart-card">
              <div className="section-heading">
                <h2>
                  {t(label)} <Info size={16} />
                </h2>
                <strong>
                  <bdi>{index ? throughput == null ? '—' : `${formatBytes(throughput * GiB)}/s` : hit == null ? '—' : `${hit.toFixed(1)}%`}</bdi>
                </strong>
              </div>
              {hasSamples ? (
                <ResponsiveContainer width="100%" height={300}>
                  <AreaChart
                    data={selected.points.map(point=>({...point,ssd:point.ssd == null ? null : point.ssd*1024,hdd:point.hdd == null ? null : point.hdd*1024}))}
                    margin={{ top: 30, right: 20, left: 0, bottom: 0 }}
                  >
                    <defs>
                      <linearGradient
                        id={`fill-${index}`}
                        x1="0"
                        y1="0"
                        x2="0"
                        y2="1"
                      >
                        <stop
                          offset="0%"
                          stopColor={index ? "#93B5C6" : "#6E7F9A"}
                          stopOpacity={0.6}
                        />
                        <stop
                          offset="100%"
                          stopColor="#93B5C6"
                          stopOpacity={0.04}
                        />
                      </linearGradient>
                    </defs>
                    <CartesianGrid
                      strokeDasharray="4 5"
                      vertical={false}
                      stroke="var(--divider)"
                    />
                    <XAxis
                      dataKey="at"
                      type="number"
                      domain={[selected.start,selected.end]}
                      ticks={Array.from({length:5},(_,i)=>selected.start+windows[period]*i/4)}
                      tickFormatter={at=>dateLabel(Number(at))}
                      tick={{ fontSize: 11, fill: "var(--secondary)" }}
                      axisLine={false}
                      tickLine={false}
                    />
                    <YAxis
                      domain={index ? [0, 'auto'] : [0, 100]}
                      tickFormatter={value=>index ? Number(value).toLocaleString(language,{maximumSignificantDigits:3}) : String(value)}
                      label={index ? {value:'MiB/s',position:'insideTopLeft',offset:0,fontSize:10,fill:'var(--secondary)'} : undefined}
                      tick={{ fontSize: 11, fill: "var(--secondary)" }}
                      axisLine={false}
                      tickLine={false}
                    />
                    <Tooltip labelFormatter={at=>dateLabel(Number(at),true)} formatter={(value,name)=>[
                      `${Number(value).toFixed(name === 'hit' ? 1 : 3)}${name === 'hit' ? '%' : ' MiB/s'}`,
                      t(name === 'hit' ? 'cacheHitRate' : name === 'ssd' ? 'ssdReads' : 'hddReads')
                    ]}/>
                    <Area
                      type="monotone"
                      dataKey={index ? "ssd" : "hit"}
                      stroke="var(--primary)"
                      strokeWidth={3}
                      fill={`url(#fill-${index})`}
                      isAnimationActive={false}
                      dot={{r:2,strokeWidth:1}}
                    />
                    {index === 1 && (
                      <Area
                        type="monotone"
                        dataKey="hdd"
                        stroke="#d0a3b5"
                        fill="#FFE3E3"
                        isAnimationActive={false}
                        dot={{r:2,strokeWidth:1}}
                      />
                    )}
                  </AreaChart>
                </ResponsiveContainer>
              ) : (
                empty(<ChartNoAxesCombined />, "noTelemetry", "performanceNoHistory")
              )}
            </Card>
          ))}
        </div>
        <div className="six-grid">
          {(
            [
              "cacheHits",
              "cacheMisses",
              "dataServed",
              "sourceRead",
              "occupancy",
              "evictions",
            ] as Key[]
          ).map((label, i) => (
            <Metric
              key={label}
              icon={i % 2 ? <Activity /> : <Database />}
              label={label}
              value={
                i === 4 ? runtime?.cacheUsedBytes == null ? undefined : formatBytes(runtime.cacheUsedBytes)
                  : hasSamples ? [String(total.hits),String(total.misses),formatBytes(total.ssdBytes),formatBytes(total.sourceBytes),undefined,String(total.evictions)][i] : undefined
              }
              caption={i === 4 ? t('performanceCurrent') : t('performanceInPeriod')}
              tint={i > 3 ? "pink" : "blue"}
            />
          ))}
        </div>

      </>
    );
  }
  function Storage() {
    return (
      <>
        {header("storageTitle", "storageDescription")}
        {data?.disks.length ? (
          data.disks.map((disk) => (
            <Card key={disk.id} className="storage-card">
              <Hardware kind={disk.kind} bus={disk.bus} />
              <div className="storage-details">
                <div className="section-heading">
                  <div>
                    <h2>
                      <bdi>
                        {disk.volumes.map((v) => v.letter).join(", ")}{" "}
                        {disk.model}
                      </bdi>
                    </h2>
                    <p>
                      <bdi>
                        {formatBytes(disk.size)} {disk.bus} {disk.kind}
                      </bdi>
                    </p>
                  </div>
                  <span className="chips">
                    <span>{disk.online ? t("healthy") : t("notRunning")}</span>
                  </span>
                </div>
                {disk.volumes.map((volume) => (
                  <div className="volume-row" key={volume.letter}>
                    <div className="capacity-note">
                      <span>
                        <bdi>
                          {volume.letter} {volume.label}
                        </bdi>{" "}
                        · {t(volume.system ? "systemVolume" : "dataVolume")}
                      </span>
                      <bdi>
                        {formatBytes(volume.size - volume.free)} /{" "}
                        {formatBytes(volume.size)}
                      </bdi>
                    </div>
                    <div className="storage-bar">
                      <span
                        style={{
                          width: `${(1 - volume.free / volume.size) * 100}%`,
                        }}
                      />
                    </div>
                    <div className="legend">
                      <span>
                        <i />
                        {t("normalFiles")}:{" "}
                        <bdi>{formatBytes(volume.size - volume.free)}</bdi>
                      </span>
                      <span>
                        <i className="empty" />
                        {t("free")}: <bdi>{formatBytes(volume.free)}</bdi>
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </Card>
          ))
        ) : (
          <Card>{empty(<Database />, "noDisks", "desktopRequired")}</Card>
        )}
        <Card className="storage-total">
          <div className="icon-orb">
            <Database />
          </div>
          <div>
            <h2>{t("totalStorage")}</h2>
            <p>
              <bdi>{data?.disks.length || 0}</bdi> {t("acrossDrives")}
            </p>
          </div>
          <div>
            <strong>
              <bdi>{formatBytes(total)}</bdi>
            </strong>
            <span>{t("capacity")}</span>
          </div>
          <div>
            <strong>
              <bdi>{formatBytes(free)}</bdi>
            </strong>
            <span>{t("totalFree")}</span>
          </div>
          <div>
            <strong>
              <bdi>{total ? Math.round((1 - free / total) * 100) : 0}%</bdi>
            </strong>
            <span>{t("overallUsage")}</span>
          </div>
        </Card>
      </>
    );
  }
  function Performance(){return <>{PerformanceCharts()}<details className="glass performance-diagnostics" open={diagnosticsOpen} onToggle={event=>setDiagnosticsOpen(event.currentTarget.open)}><summary><Activity size={19}/>{t('diagnosticsTitle')}<ChevronDown size={18}/></summary>{Diagnostics()}</details></>}
  function Diagnostics() {
    const folderEngine=['dokanFolder','dokanLibrary'].includes(runtime?.backend||'');
    return (
      <>
        <div className="section-heading"><p>{t('diagnosticsDescription')}</p><button
            className="secondary"
            onClick={() => void refresh(true)}
            disabled={loading}
          >
            <RefreshCw size={18} className={loading ? "spin" : ""} />
            {t("retry")}
          </button></div>
        <div className="two-grid">
          <Card className="health-card">
            <div className="icon-orb">
              <Shield />
            </div>
            <h2>{t("diagnosticsEngine")}</h2>
            <strong>{folderEngine&&runtime ? t(`runtimePhase_${runtime.phase}`) : telemetry?.attached ? t(cacheStateKey) : driverActive ? t("running") : t("notRunning")}</strong>
            <p>{t(folderEngine ? "diagnosticsFolderEngine" : telemetry?.attached ? "diagnosticsConnected" : "diagnosticsUnavailable")}</p>
            {telemetry && <div data-testid="engine-diagnostics">
              <p>{t("driverProtocol")}: <bdi>{telemetry.protocolVersion}</bdi></p>
              <p>{t("pendingCacheReads")}: <bdi>{telemetry.pendingReads} / {telemetry.queueLimit}</bdi></p>
              <p>{t("inflightMutations")}: <bdi>{telemetry.activeMutations}</bdi></p>
              <p>{t("bypassedReads")}: <bdi>{telemetry.readBypasses}</bdi></p>
              <p>{t("diagnosticsErrors")}: <bdi>{telemetry.errors}</bdi></p>
              <p>{t("allocated")}: <bdi>{formatBytes(telemetry.capacityBytes)}</bdi></p>
              {telemetry.nativeVolume && <div data-testid="native-volume-diagnostics">
                <hr />
                <h3>{t("nativeVolumeEngine")}</h3>
                <p>{t("sourceDiskNumber")}: <bdi>{telemetry.nativeVolume.sourceDiskNumber ?? '—'}</bdi></p>
                <p>{t("cacheDiskNumber")}: <bdi>{telemetry.nativeVolume.backingDiskNumber ?? '—'}</bdi></p>
                <p>{t("totalCacheAllocation")}: <bdi>{formatBytes(telemetry.nativeVolume.allocatedBytes)}</bdi></p>
                <p>{t("pagingCacheHits")}: <bdi>{telemetry.nativeVolume.pagingHits}</bdi></p>
                <p>{t("cancelledCacheReads")}: <bdi>{telemetry.nativeVolume.cancelledReads}</bdi></p>
                <p>{t("revalidatedCacheRecords")}: <bdi>{telemetry.nativeVolume.revalidatedRecords}</bdi></p>
              </div>}
            </div>}
          </Card>
          <Card className="health-card">
            <div className="icon-orb">
              <Layers />
            </div>
            <h2>{t(folderEngine?"diagnosticsHost":"serviceStatus")}</h2>
            <strong>
              <bdi>{folderEngine&&runtime?t(`runtimePhase_${runtime.phase}`):data?.serviceState || t("noService")}</bdi>
            </strong>
            {folderEngine && <p>{t("diagnosticsHostHint")}</p>}
          </Card>
          <Card className="health-card">
            <div className="icon-orb green">
              <ShieldCheck />
            </div>
            <h2>{t("cacheStrategy")}</h2>
            <p>{t("cacheStrategyHint")}</p>
            {folderEngine&&<><hr/><h3>{t("diagnosticsManagement")}</h3><p>{t("diagnosticsManagementHint")}</p><button className="secondary" onClick={()=>go('library')}><Gamepad2 size={18}/>{t('library')}</button></>}
          </Card>
        </div>
        <Card className="scan-time">
          <Clock size={17} />
          {t("snapshotTime")}: <bdi>{data?.collectedAt || "—"}</bdi>
        </Card>
      </>
    );
  }
  function SettingsPage() {
    return (
      <>
        {header("settingsTitle", "settingsDescription")}
        <Card className="settings-panel">
          <div className="segmented settings-tabs">
            {(["general", "appearance", "language"] as Key[]).map((tab) => (
              <button
                key={tab}
                className={settingsTab === tab ? "active" : ""}
                aria-pressed={settingsTab === tab}
                onClick={() => setSettingsTab(tab)}
              >
                {t(tab)}
              </button>
            ))}
          </div>
          <div className="settings-body">
            <div className="settings-rows">
              {settingsTab === "general" ? (
                <>
                  <div className="setting-row disabled">
                    <div className="icon-orb">
                      <Square />
                    </div>
                    <span>
                      <strong>{t("startWindows")}</strong>
                      <small>{t("startWindowsHint")}</small>
                    </span>
                    <Toggle
                      checked={false}
                      disabled
                      label={t("startWindows")}
                    />
                  </div>
                  <div className="setting-row disabled">
                    <div className="icon-orb pink">
                      <Info />
                    </div>
                    <span>
                      <strong>{t("notifications")}</strong>
                      <small>{t("notificationsHint")}</small>
                    </span>
                    <Toggle
                      checked={false}
                      disabled
                      label={t("notifications")}
                    />
                  </div>
                  <div className="setting-row">
                    <div className="icon-orb">
                      <Activity />
                    </div>
                    <span>
                      <strong>{t("reducedMotion")}</strong>
                      <small>{t("reducedMotionHint")}</small>
                    </span>
                    <Toggle
                      checked={reduced}
                      onChange={() => setReduced(!reduced)}
                      label={t("reducedMotion")}
                    />
                  </div>
                </>
              ) : settingsTab === "appearance" ? (
                <div className="setting-row">
                  <div className="icon-orb">
                    {theme === "dark" ? <Moon /> : <Sun />}
                  </div>
                  <span>
                    <strong>{t("theme")}</strong>
                  </span>
                  <select
                    aria-label={t("theme")}
                    className="input"
                    value={theme}
                    onChange={(e) => setTheme(e.target.value)}
                  >
                    <option value="light">{t("light")}</option>
                    <option value="dark">{t("dark")}</option>
                  </select>
                </div>
              ) : (
                <div className="setting-row">
                  <div className="icon-orb">
                    <Languages />
                  </div>
                  <span>
                    <strong>{t("language")}</strong>
                  </span>
                  <select
                    aria-label={t("language")}
                    className="input"
                    value={language}
                    onChange={(e) => setLanguage(e.target.value as "ar" | "en")}
                  >
                    <option value="en">English</option>
                    <option value="ar">العربية</option>
                  </select>
                </div>
              )}
            </div>
            <div className="settings-art">
              <Settings size={180} strokeWidth={1} />
              <h3>{t("modernExperience")}</h3>
              <p>{t("beautifulSimple")}</p>
            </div>
          </div>
        </Card>
      </>
    );
  }

  return (
    <div className={`app-shell ${welcome ? "welcome-shell" : ""}`}>
      <aside className="sidebar">
        <button
          className="brand"
          onClick={() => go("home")}
          aria-label="Super HDD"
        >
          <span className="brand-icon">
            <img src="/assets/brand-icon.png" alt="" aria-hidden="true" />
          </span>
          <strong>Super HDD</strong>
        </button>
        {!welcome && (
          <nav aria-label={t("settings")}>
            {nav.map(({ id, icon: Icon }) => (
              <button
                key={id}
                aria-current={page === id ? "page" : undefined}
                className={page === id ? "selected" : ""}
                onClick={() => go(id)}
              >
                <Icon size={22} />
                <span>{t(id)}</span>
              </button>
            ))}

          </nav>
        )}
        <div className="sidebar-bottom">
          <span className={`connection ${api.demo ? "preview" : ""}`}>
            <i />
            {t(api.demo ? "demoLabel" : "liveLabel")}
          </span>
          <small>{t("ssdOnly")}</small>
          <button
            className="text-button"
            onClick={() => setLanguage(language === "en" ? "ar" : "en")}
          >
            <Languages size={16} />
            {language === "en" ? "العربية" : "English"}
          </button>
        </div>
      </aside>
      <div className="window-drag" data-tauri-drag-region />
      <div className="window-controls">
        <button
          className="title-settings"
          title={t("settings")}
          onClick={() => go("settings")}
        >
          <Settings size={18} />
        </button>
        <span />
        <button
          title="Minimize"
          aria-label="Minimize"
          onClick={() => void api.windowAction("minimize")}
        >
          <Minus size={18} />
        </button>
        <button
          title="Maximize"
          aria-label="Maximize"
          onClick={() => void api.windowAction("toggleMaximize")}
        >
          <Square size={15} />
        </button>
        <button
          title={t("close")}
          aria-label={t("close")}
          className="window-close"
          onClick={() => void api.windowAction("close")}
        >
          <X size={20} />
        </button>
      </div>
      <main>
        {api.demo && (
          <div className="demo-banner">
            <Info size={15} />
            {t("demoBanner")}
          </div>
        )}
        {error && (
          <div role="alert" className="error-banner">
            <AlertCircle size={18} />
            <span>{t(error)}</span>
            <button
              className="text-button"
              onClick={() => setError("")}
              aria-label={t("close")}
            >
              <X size={16} />
            </button>
          </div>
        )}
        {welcome ? (
          <div className="welcome">
            <div className="welcome-copy">
              <span className="eyebrow">{t("tagline")}</span>
              <h1>
                {t("welcomeTitle")}
                <br />
                <span>{t("welcomeAccent")}</span>
              </h1>
              <p>{t("welcomeText")}</p>
              <button className="primary get-started" onClick={start}>
                {t("getStarted")}
                <ArrowRight size={24} />
              </button>
            </div>
            <img
              className="welcome-art"
              src="/assets/brand-logo.png"
              alt="SuperHDD"
            />
            <Card className="welcome-features">
              {(
                [
                  ["realAcceleration", "ssdPowered", Zap],
                  ["simpleSetup", "oneClearFlow", MousePointer2],
                  ["safeReliable", "sourceAuthoritative", ShieldCheck],
                  ["modernExperience", "beautifulSimple", Heart],
                ] as const
              ).map(([title, subtitle, Icon], i) => (
                <div key={title}>
                  <div className={`icon-orb ${i > 1 ? "pink" : ""}`}>
                    <Icon size={29} />
                  </div>
                  <span>
                    <strong>{t(title)}</strong>
                    <small>{t(subtitle)}</small>
                  </span>
                </div>
              ))}
            </Card>
          </div>
        ) : (
          <div className="page-content" key={page}>
            {loading && !data ? (
              <div className="loading-state" role="status">
                <RefreshCw size={30} className="spin" />
                <p>{t("loading")}</p>
              </div>
            ) : page === "library" ? (
              <AppLibrary language={language} onSettings={()=>go('acceleration')}/>
            ) : page === "acceleration" ? (
              Setup()
            ) : page === "performance" ? (
              Performance()
            ) : page === "storage" ? (
              Storage()
            ) : page === "settings" ? (
              SettingsPage()
            ) : (
              Dashboard()
            )}
          </div>
        )}
      </main>
      {modal && (
        <div
          className="modal-backdrop"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setModal(null);
          }}
        >
          <section
            className="modal glass"
            role="dialog"
            aria-modal="true"
            aria-labelledby="modal-title"
            tabIndex={-1}
          >
            <button
              className="icon-button modal-close"
              aria-label={t("close")}
              onClick={() => setModal(null)}
            >
              <X />
            </button>
            <h2 id="modal-title">
              {t("planTitle")}
            </h2>
            {error && <p role="alert" className="error-text">{t(error)}</p>}
            <>
                <p>{t("planHint")}</p>
                <div className="plan-details">
                  <span>
                    {t("sourceHdd")}
                    <bdi>{plan?.request.sourceLetter}</bdi>
                  </span>
                  <span>
                    {t("cacheSsd")}
                    <bdi>{plan?.request.ssdLetter}</bdi>
                  </span>
                  <span>
                    {t("cacheSize")}
                    <bdi>
                      {plan ? `${(plan.request.capacityBytes / GB).toFixed(1)} GB` : "—"}
                    </bdi>
                  </span>
                </div>
                <div className="plan-scope">
                  <strong>{t("cacheScope")}: {t(plan?.request.scope ? "selectedOnly" : "wholeVolume")}</strong>
                  {plan?.request.scope?.folders.map(path => <p key={path}><bdi dir="ltr">{path}</bdi></p>)}
                  {plan?.request.scope && <p>{t(['dokanFolder','dokanLibrary'].includes(runtime?.backend||'')?"folderScopeHint":"selectedScopeUnavailable")}</p>}
                </div>
                {plan?.activationAvailable&&runtime?.canStart&&(!plan.request.scope||['dokanFolder','dokanLibrary'].includes(runtime.backend||''))?<button className="primary wide" disabled={busy} onClick={()=>void operateCache('start')}>{t(busy?'loading':'runtimeStart')}</button>:<div className="blocked-label">
                  <Shield size={20} />
                  {t("planBlocked")}
                </div>}
                <ul>
                  {plan?.reasons.map((reason) => (
                    <li key={reason}>{t(reason)}</li>
                  ))}
                  {runtime?.reason&&!plan?.reasons.includes(runtime.reason)&&<li>{t(runtime.reason)}</li>}
                </ul>
                <button className="primary wide" onClick={() => setModal(null)}>
                  {t("close")}
                </button>
            </>
          </section>
        </div>
      )}
    </div>
  );
}
