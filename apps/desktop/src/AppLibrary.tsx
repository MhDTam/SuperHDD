import {useCallback,useEffect,useState} from 'react';
import './library-scope.css';
import {Gamepad2,AppWindow,Search,RefreshCw,Folder,Play,Pause,Trash2,Square,Plus,Info,ChevronDown,File,Settings2,ArrowRight} from 'lucide-react';
import * as api from './api';
import CacheConfigurations from './CacheConfigurations';
import type {AppLibrary as LibraryState,LibraryApp,LibraryCommand,Snapshot} from './types';
import {formatBytes as size} from './format-bytes';
import en from './locales/en.json';
import ar from './locales/ar.json';
import libraryEn from './locales/library-en.json';
import libraryAr from './locales/library-ar.json';
type Translate=(key:string)=>string;
function LibraryIcon({app}:{app:LibraryApp}){
 const [failedSource,setFailedSource]=useState<string|null>(null);
 const source=app.iconDataUri;
 return source&&source!==failedSource?<img className="library-icon" src={source} alt="" onError={()=>setFailedSource(source)}/>:<div className="library-icon" aria-hidden="true">{app.kind==='game'?<Gamepad2/>:<AppWindow/>}</div>;
}
function CachedFiles({app,t}:{app:LibraryApp;t:Translate}){
 const [query,setQuery]=useState('');const [sort,setSort]=useState('largest');const [limit,setLimit]=useState(25);
 const files=app.cachedFiles||[];
 const relative=(path:string)=>path.toLowerCase().startsWith(app.path.toLowerCase()+'\\')?path.slice(app.path.length+1):path;
 const matches=files.filter(file=>relative(file.path).toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a,b)=>sort==='name'?relative(a.path).localeCompare(relative(b.path)):b.residentBytes-a.residentBytes||a.path.localeCompare(b.path));
 return <details className="library-files">
  <summary><File size={17}/><span>{t('libraryFiles')}</span><bdi className="library-file-count">{files.length}</bdi><ChevronDown size={16}/></summary>
  <div className="library-file-controls"><label className="library-search"><Search size={16}/><input className="input" aria-label={t('libraryFileSearch')} placeholder={t('libraryFileSearch')} value={query} onChange={e=>{setQuery(e.target.value);setLimit(25)}}/></label><select className="input" aria-label={t('libraryFileSort')} value={sort} onChange={e=>{setSort(e.target.value);setLimit(25)}}><option value="largest">{t('libraryLargest')}</option><option value="name">{t('libraryByName')}</option></select></div>
  <p className="library-file-hint">{t('libraryResidentHint')}</p>
  {matches.length?<ul className="library-file-list" aria-label={t('libraryFiles')}>{matches.slice(0,limit).map(file=>{const path=relative(file.path);const split=path.lastIndexOf('\\');return <li key={file.path}><File size={17}/><span><bdi dir="ltr" title={file.path}>{path.slice(split+1)}</bdi>{split>=0&&<small><bdi dir="ltr">{path.slice(0,split)}</bdi></small>}</span><bdi className="library-file-size">{size(file.residentBytes)}</bdi></li>})}</ul>:<p className="library-files-empty">{t(files.length?'libraryNoFilesMatch':'folderCacheEmpty')}</p>}
  {matches.length>limit&&<button className="text-button" onClick={()=>setLimit(value=>value+25)}>{t('libraryMoreFiles')} <bdi>({matches.length-limit})</bdi></button>}
 </details>;
}
interface Props {language:'en'|'ar';settingsOnly?:boolean;snapshot?:Snapshot|null;onSettings?:()=>void;onLibrary?:()=>void;onRefresh?:()=>void}
export default function AppLibrary({language,settingsOnly=false,snapshot,onSettings,onLibrary,onRefresh}:Props){
 const strings={...(language==='ar'?ar:en),...(language==='ar'?libraryAr:libraryEn)};const t:Translate=key=>(strings as Record<string,string>)[key]||key;
 const [data,setData]=useState<LibraryState|null>(null);const [loading,setLoading]=useState(true);const [error,setError]=useState('');const [busy,setBusy]=useState('');
 const [search,setSearch]=useState('');const [filter,setFilter]=useState('all');const [configurationFilter,setConfigurationFilter]=useState('all');const [view,setView]=useState('managed');const [otherDrives,setOtherDrives]=useState(false);
 const [manual,setManual]=useState(false);const [path,setPath]=useState('');const [name,setName]=useState('');
 const load=useCallback(async(refresh=false)=>{try{const next=await api.appLibrary(refresh);setData(next);return next}catch(e){setError(String(e));return null}finally{setLoading(false)}},[]);
 useEffect(()=>{if(settingsOnly)return;void load();const timer=setInterval(()=>{if(!document.hidden)void load()},4000);return()=>clearInterval(timer)},[load,settingsOnly]);
 async function act(command:LibraryCommand){if(busy)return;setBusy(command.id||'budget');setError('');try{await api.libraryOperation(command);await load()}catch(e){setError(String(e))}finally{setBusy('')}}
 async function add(){if(busy||!path.trim())return;setBusy('manual');setError('');try{await api.addLibraryFolder(path.trim(),name.trim()||null);setPath('');setName('');setManual(false);setView('managed');await load()}catch(e){setError(String(e))}finally{setBusy('')}}
 async function browse(){setError('');try{const selected=await api.pickAppFolder();if(selected)setPath(selected)}catch(e){setError(String(e))}}
 const all=data?.apps||[];const live=all.filter(app=>app.phase!=='stopped');const used=data?.cacheUsedBytes??live.reduce((sum,app)=>sum+app.cacheUsedBytes,0);const total=data?.budgetBytes||0;
 const eligible=all.filter(app=>otherDrives||app.eligible||app.managed);const managedCount=eligible.filter(app=>app.managed).length;
 const apps=eligible.filter(app=>(view==='all'||app.managed)&&(filter==='all'||app.kind===filter)&&(configurationFilter==='all'||app.configurationId===configurationFilter)&&`${app.name} ${app.path}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a,b)=>Number(b.phase!=='stopped')-Number(a.phase!=='stopped')||Number(b.managed)-Number(a.managed)||a.name.localeCompare(b.name));
 const overall=live.some(app=>app.phase==='faulted')?'faulted':live.some(app=>app.phase==='starting')?'starting':!live.length?'stopped':live.every(app=>app.phase==='paused')?'paused':'active';
 const percent=(value:number,capacity:number)=>capacity>0?Math.min(100,Math.max(0,value/capacity*100)):0;
 function buttons(app:LibraryApp){const disabled=!!busy||!app.eligible||!data?.available;return <div className="library-actions">
  {app.phase==='stopped'?<button className="primary" disabled={disabled||data?.legacyActive} onClick={()=>void act({operation:'start',id:app.id,app})}><Play size={16}/>{t('libraryStart')}</button>:<>
   {['active','paused'].includes(app.phase)&&<button className="secondary" disabled={!!busy} onClick={()=>void act({operation:app.phase==='paused'?'resume':'pause',id:app.id})}>{app.phase==='paused'?<Play size={16}/>:<Pause size={16}/>} {t(app.phase==='paused'?'runtimeResume':'runtimePause')}</button>}
   <button className="secondary" disabled={!!busy||!['active','paused'].includes(app.phase)} onClick={()=>void act({operation:'clear',id:app.id})}><Trash2 size={16}/>{t('libraryClear')}</button>
   <button className="text-button library-stop" disabled={!!busy||app.phase==='starting'} onClick={()=>void act({operation:'stop',id:app.id})}><Square size={15}/>{t(app.phase==='faulted'?'libraryRestore':'libraryStop')}</button>
  </>}
  {app.managed&&app.phase==='stopped'&&<button className="icon-button" aria-label={`${t('libraryRemove')} ${app.name}`} disabled={!!busy} onClick={()=>void act({operation:'remove',id:app.id})}><Trash2 size={16}/></button>}
 </div>}
 const notices=<>{error&&<p className="error-text" role="alert">{t(error)}</p>}{data?.legacyActive&&<div className="glass library-notice"><Info size={18}/><p>{t('libraryLegacyActive')}</p></div>}{data&&!data.available&&<div className="glass library-notice"><Info size={18}/><p>{t('folderRuntimeUnavailable')}</p></div>}</>;
 if(settingsOnly)return <CacheConfigurations language={language} snapshot={snapshot} onLibrary={onLibrary} onRefresh={onRefresh}/>;
 return <>
  <header className="page-header"><div><h1>{t('libraryTitle')}</h1><p>{t('libraryDescription')}</p></div><div className="library-toolbar"><button className="secondary" disabled={!!busy||loading} onClick={()=>{setLoading(true);setError('');void load(true)}}><RefreshCw size={18} className={loading?'spin':''}/>{t('libraryRefresh')}</button><button className="primary" onClick={()=>setManual(value=>!value)}><Plus size={18}/>{t('libraryAddFolder')}</button></div></header>
  <section className="glass library-overview" aria-label={t('librarySummary')}>
   <div className="library-overview-heading"><h2>{t('librarySummary')}</h2><button className="text-button" onClick={onSettings}><Settings2 size={17}/>{t('acceleration')}<ArrowRight size={16}/></button></div>
   <dl className="library-summary-facts"><div><dt>{t('libraryCapacity')}</dt><dd><bdi>{data?size(total):'—'}</bdi></dd></div><div><dt>{t('cacheUsed')}</dt><dd><bdi>{data?size(used):'—'}</bdi></dd></div><div><dt>{t('free')}</dt><dd><bdi>{data?size(Math.max(0,total-used)):'—'}</bdi></dd></div><div><dt>{t('libraryStatus')}</dt><dd><span className={`library-state ${overall}`} role="status">{data?t('runtimePhase_'+overall):'—'}</span><small>{live.length} {t('libraryActiveItems')}</small></dd></div></dl>
   <div className="library-usage" role="progressbar" aria-label={t('libraryUsage')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Number(percent(used,total).toFixed(2))}><span style={{width:`${percent(used,total)}%`}}/></div><p>{t('libraryBudgetHint')}</p>{data?.cachePolicy==='loadingPriority'&&<p className="library-loading-policy"><strong>{t('libraryLoadingTitle')}</strong> {t('libraryLoadingHint')}</p>}
  </section>
  {manual&&<form className="glass library-manual" onSubmit={e=>{e.preventDefault();void add()}}><h2>{t('libraryAddFolder')}</h2><label>{t('libraryOptionalName')}<input className="input" value={name} onChange={e=>setName(e.target.value)}/></label><label>{t('appFolder')}<div className="library-path-entry"><input className="input" dir="ltr" value={path} onChange={e=>setPath(e.target.value)} placeholder="D:\Games\My game"/><button className="secondary" type="button" onClick={()=>void browse()}><Folder size={17}/>{t('libraryBrowse')}</button></div></label><p>{t('librarySubfolders')}</p><button className="primary" disabled={!!busy||!path.trim()}>{t('libraryAdd')}</button></form>}
  {notices}
  <div className="library-view-tabs"><button aria-pressed={view==='managed'} onClick={()=>setView('managed')}>{t('libraryMyCache')} <bdi>{managedCount}</bdi></button><button aria-pressed={view==='all'} onClick={()=>setView('all')}>{t('libraryInstalled')} <bdi>{eligible.length}</bdi></button></div>
  <div className="library-filters"><label className="library-search"><Search size={18}/><input className="input" aria-label={t('librarySearch')} placeholder={t('librarySearch')} value={search} onChange={e=>setSearch(e.target.value)}/></label><select className="input" aria-label={t('libraryFilter')} value={filter} onChange={e=>setFilter(e.target.value)}>{['all','game','app'].map(key=><option key={key} value={key}>{t('libraryFilter_'+key)}</option>)}</select>{(data?.configurations?.length||0)>1&&<select className="input" aria-label={t('configFilter')} value={configurationFilter} onChange={e=>setConfigurationFilter(e.target.value)}><option value="all">{t('configAll')}</option>{data?.configurations?.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select>}{view==='all'&&<label><input type="checkbox" checked={otherDrives} onChange={e=>setOtherDrives(e.target.checked)}/>{t('libraryOtherDrives')}</label>}</div>
  {loading&&!data?<div className="loading-state" role="status"><RefreshCw className="spin"/>{t('libraryScanning')}</div>:<div className="library-grid">{apps.map(app=><section className={`glass library-app ${app.phase!=='stopped'?'enabled':''}`} key={app.id} data-testid="library-app" aria-label={app.name}>
   <div className="library-app-heading"><LibraryIcon app={app}/><div><h2>{app.name}</h2><small>{app.origin==='Manual'?t('libraryManual'):app.origin}</small></div><span className={`library-state ${app.phase}`} role="status">{t('runtimePhase_'+app.phase)}</span></div>
   <bdi className="library-app-path" dir="ltr" title={app.path}>{app.path}</bdi><p className="library-includes">{t(app.cacheMode==='dataOnly'?'libraryDataScopeIncludes':'librarySubfolders')}{app.sourceLetter&&app.ssdLetter&&<> · <bdi dir="ltr">{app.sourceLetter} → {app.ssdLetter}</bdi></>}</p>
   {app.kind==='game'&&<div className="library-cache-mode"><label><span>{t('libraryCacheMode')}</span><select className="input" aria-label={`${t('libraryCacheMode')} ${app.name}`} value={app.cacheMode||'full'} disabled={!!busy||app.phase!=='stopped'} onChange={e=>void act({operation:e.target.value==='dataOnly'?'modeData':'modeFull',id:app.id,app})}><option value="dataOnly" disabled={!app.dataCacheAvailable}>{t('libraryDataOnly')}</option><option value="full">{t('libraryWholeFolder')}</option></select></label><p>{t(app.cacheMode==='dataOnly'?'libraryDataOnlyHint':'libraryWholeFolderHint')}</p>{app.cacheMode==='dataOnly'&&app.dataCachePath&&<bdi dir="ltr" className="library-data-path" title={app.dataCachePath}>{app.dataCachePath.toLowerCase().startsWith(app.path.toLowerCase()+'\\')?app.dataCachePath.slice(app.path.length+1):app.dataCachePath}</bdi>}{!app.dataCacheAvailable&&<small>{t('libraryDataNotFound')}</small>}{app.phase!=='stopped'&&<small>{t('libraryStopModeFirst')}</small>}</div>}
   {app.managed&&<><dl className="library-app-facts"><div><dt>{t('cacheUsed')}</dt><dd><bdi>{size(app.cacheUsedBytes)}</bdi>{app.sharedPool?<small className="shared-pool-note">{t('libraryUsesSharedPool')}</small>:<small> / <bdi>{size(app.allocationBytes)}</bdi></small>}</dd></div><div><dt>{t('libraryFiles')}</dt><dd><bdi>{(app.cachedFiles||[]).length}</bdi></dd></div></dl>{!app.sharedPool&&<div className="library-usage" role="progressbar" aria-label={t('libraryUsage')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Number(percent(app.cacheUsedBytes,app.allocationBytes).toFixed(2))}><span style={{width:`${percent(app.cacheUsedBytes,app.allocationBytes)}%`}}/></div>}</>}
   {!app.eligible&&<p>{t(app.reason||'configChooseDrive')}</p>}{buttons(app)}{busy===app.id&&<small className="library-progress"><RefreshCw size={15} className="spin"/>{t('libraryWorking')}</small>}
   {app.managed&&app.phase!=='stopped'&&<CachedFiles app={app} t={t}/>}
  </section>)}</div>}
  {!loading&&!apps.length&&<section className="glass library-empty"><Gamepad2 size={40}/><h2>{t('libraryNoResults')}</h2><p>{t('libraryManualHint')}</p>{view==='managed'&&<button className="secondary" onClick={()=>setView('all')}>{t('libraryInstalled')}</button>}</section>}
  <p className="library-footer"><Info size={17}/>{t('libraryClearHint')}</p>
 </>;
}
