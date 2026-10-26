import {useCallback,useEffect,useState,useRef} from 'react';
import {Plus,Settings2,Square,Trash2,HardDrive,Gamepad2,X,ShieldCheck,RefreshCw,ArrowRight,Database,Layers} from 'lucide-react';
import * as api from './api';
import type {AppLibrary,CacheConfiguration,ConfigurationCommand,Snapshot} from './types';
import {formatBytes} from './format-bytes';
import en from './locales/en.json';import ar from './locales/ar.json';import libraryEn from './locales/library-en.json';import libraryAr from './locales/library-ar.json';
export default function CacheConfigurations({language,snapshot,onLibrary,onRefresh}:{language:'en'|'ar';snapshot?:Snapshot|null;onLibrary?:()=>void;onRefresh?:()=>void}){
 const strings={...(language==='ar'?ar:en),...(language==='ar'?libraryAr:libraryEn)};const t=(key:string)=>(strings as Record<string,string>)[key]||key;
 const [data,setData]=useState<AppLibrary|null>(null);const [error,setError]=useState('');const [busy,setBusy]=useState(false);
 const [budgetText,setBudgetText]=useState('25');
 const editorRef=useRef<HTMLFormElement>(null);
 const editorTrigger=useRef<HTMLButtonElement|null>(null);
 const [editor,setEditor]=useState<CacheConfiguration|null>(null);const [remove,setRemove]=useState<string|null>(null);
 const load=useCallback(async()=>{try{setData(await api.appLibrary())}catch(e){setError(String(e))}},[]);
 useEffect(()=>{void load();const timer=setInterval(()=>{if(!document.hidden)void load()},4000);return()=>clearInterval(timer)},[load]);
 const configurations=data?.configurations||[];
 const options=(kind:string)=>(snapshot?.disks||[]).filter(d=>d.kind===kind&&d.online&&!d.readOnly).flatMap(d=>d.volumes.filter(v=>!v.system&&v.fileSystem==='NTFS'&&v.health==='Healthy').map(v=>({disk:d,volume:v,key:JSON.stringify([d.id,v.letter])})));
 const sources=options('HDD');const ssds=options('SSD');
 function add(){setError('');setBudgetText('25');const a=sources.find(o=>!configurations.some(c=>c.sourceDiskId===o.disk.id&&c.sourceLetter===o.volume.letter))||sources[0];const b=ssds[0];setEditor({id:'',name:'',sourceDiskId:a?.disk.id||'',sourceLetter:a?.volume.letter||'',ssdDiskId:b?.disk.id||'',ssdLetter:b?.volume.letter||'',budgetBytes:24999952384})}
 function edit(c:CacheConfiguration){setError('');setBudgetText(String(Math.round(c.budgetBytes/1e7)/100));setEditor({id:c.id,name:c.name,sourceDiskId:c.sourceDiskId,sourceLetter:c.sourceLetter,ssdDiskId:c.ssdDiskId,ssdLetter:c.ssdLetter,budgetBytes:c.budgetBytes})}
 async function operate(command:ConfigurationCommand){if(busy)return;setBusy(true);setError('');try{await api.configurationOperation(command);setEditor(null);setRemove(null);await load()}catch(e){setError(String(e));await load()}finally{setBusy(false)}}
 useEffect(()=>{if(editor){editorRef.current?.scrollIntoView({block:'nearest'});editorRef.current?.querySelector<HTMLInputElement>('input')?.focus({preventScroll:true})}},[editor?.id]);
 function closeEditor(){setEditor(null);editorTrigger.current?.focus()}
 const editingActive=!!editor?.id&&!!configurations.find(c=>c.id===editor.id)?.activeCount;
 return <>
  <header className="page-header configuration-page-header"><div><h1>{t('acceleration')}</h1><p>{t('configDescription')}</p></div><button className="primary" disabled={busy||configurations.length>=8} onClick={event=>{editorTrigger.current=event.currentTarget;add()}}><Plus size={18}/>{t('configAdd')}</button></header>
  {error&&<p className="error-text" role="alert">{t(error)}</p>}
  {editor&&<form ref={editorRef} className="glass configuration-editor" aria-label={t('configEditor')} onSubmit={event=>{event.preventDefault();void operate({operation:'save',configuration:editor})}}>
   <div className="section-heading"><h2><Settings2 size={20}/>{t(editor.id?'configEdit':'configAdd')}</h2><button className="icon-button" type="button" disabled={busy} aria-label={t('close')} onClick={closeEditor}><X size={18}/></button></div>
   <div className="two-grid configuration-editor-basics"><label>{t('configName')}<input className="input" value={editor.name} maxLength={160} onChange={e=>setEditor({...editor,name:e.target.value})}/></label><label>{t('configBudgetLabel')}<input className="input" type="number" dir="ltr" min="1.1" max="25" step="0.1" value={budgetText} onChange={e=>{setBudgetText(e.target.value);setEditor({...editor,budgetBytes:Math.floor(Number(e.target.value)*1e9/65536)*65536})}}/></label></div>
   <div className="two-grid"><label>{t('sourceHdd')}<select className="input" value={JSON.stringify([editor.sourceDiskId,editor.sourceLetter])} disabled={busy||editingActive} onChange={e=>{const [sourceDiskId,sourceLetter]=JSON.parse(e.target.value);setEditor({...editor,sourceDiskId,sourceLetter})}}>
    {!sources.some(o=>o.disk.id===editor.sourceDiskId&&o.volume.letter===editor.sourceLetter)&&<option value={JSON.stringify([editor.sourceDiskId,editor.sourceLetter])}>{t('configSelectHdd')}</option>}
    {sources.map(o=><option key={o.key} value={o.key} disabled={configurations.some(c=>c.id!==editor.id&&c.sourceLetter===o.volume.letter)}>{o.volume.letter} {o.disk.model}</option>)}
   </select></label><label>{t('cacheSsd')}<select className="input" value={JSON.stringify([editor.ssdDiskId,editor.ssdLetter])} disabled={busy||editingActive} onChange={e=>{const [ssdDiskId,ssdLetter]=JSON.parse(e.target.value);setEditor({...editor,ssdDiskId,ssdLetter})}}>
    {!ssds.some(o=>o.disk.id===editor.ssdDiskId&&o.volume.letter===editor.ssdLetter)&&<option value={JSON.stringify([editor.ssdDiskId,editor.ssdLetter])}>{t('configSelectSsd')}</option>}
    {ssds.map(o=><option key={o.key} value={o.key}>{o.volume.letter} {o.disk.model} · {formatBytes(o.volume.free)} {t('configAvailableCapacity')}</option>)}
   </select></label></div>

   <div className="configuration-editor-footer"><p><ShieldCheck size={17}/>{t(editingActive?'configStopFirst':'configBudgetHint')}</p><button className="primary" disabled={busy||!data?.available||!editor.sourceDiskId||!editor.ssdDiskId||editor.budgetBytes<1.1e9||editor.budgetBytes>25e9||!Number.isFinite(editor.budgetBytes)}>{t('configSave')}</button></div>
  </form>}
  <div className="configuration-section-heading"><h2>{t('configConnections')} <span>{configurations.length}</span></h2><div className="configuration-secondary-actions"><button className="text-button" onClick={onLibrary}><Gamepad2 size={17}/>{t('libraryManage')}</button><span aria-hidden="true"/><button className="text-button" disabled={busy} onClick={()=>{onRefresh?.();void load()}}><RefreshCw size={16}/>{t('retry')}</button></div></div>
  <div className="configuration-list">{configurations.map(c=>{
   const source=snapshot?.disks.find(d=>d.id===c.sourceDiskId);const ssd=snapshot?.disks.find(d=>d.id===c.ssdDiskId);
   const used=c.cacheUsedBytes||0;const percent=c.budgetBytes?Math.min(100,Math.max(0,used/c.budgetBytes*100)):0;
   const apps=(data?.apps||[]).filter(app=>app.configurationId===c.id&&app.phase!=='stopped');
   const phase=apps.some(app=>app.phase==='faulted')?'faulted':apps.some(app=>app.phase==='starting')?'starting':apps.length&&apps.every(app=>app.phase==='paused')?'paused':c.activeCount?'active':'stopped';
   const defaultName=c.name.replace(/\s/g,'')===`${c.sourceLetter}→${c.ssdLetter}`;
   return <section className="glass configuration-card" aria-label={c.name} key={c.id}>
    <div className="configuration-card-heading"><div className="configuration-identity"><span className="configuration-icon"><Layers size={21}/></span><div><h2>{defaultName?t('configSharedCache'):c.name}</h2><p>{t('configSharedSubtitle')}</p></div></div><span className={`library-state configuration-state ${phase}`} role="status"><i/>{t('runtimePhase_'+phase)}</span></div>
    <div className="configuration-main">
     <div className="configuration-route" role="group" aria-label={`${c.sourceLetter} → ${c.ssdLetter}`}>
      <div className="configuration-drive"><div className="configuration-drive-label"><HardDrive size={18}/><span>{t('sourceHdd')}</span></div><div className="configuration-drive-letter"><bdi dir="ltr">{c.sourceLetter}</bdi><span>HDD</span></div><bdi className="configuration-drive-model" title={source?.model}>{source?.model||t('configDeviceMissing')}</bdi><small>{t('configOriginalFiles')}</small></div>
      <div className="configuration-route-arrow" aria-hidden="true"><ArrowRight size={20}/></div>
      <div className="configuration-drive cache-drive"><div className="configuration-drive-label"><Database size={18}/><span>{t('cacheSsd')}</span></div><div className="configuration-drive-letter"><bdi dir="ltr">{c.ssdLetter}</bdi><span>{ssd?.bus?.toLowerCase()==='nvme'?'NVMe':'SSD'}</span></div><bdi className="configuration-drive-model" title={ssd?.model}>{ssd?.model||t('configDeviceMissing')}</bdi><small>{t('configCachedCopies')}</small></div>
     </div>
     <div className="configuration-capacity"><span>{t('configSharedCapacity')}</span><strong><bdi>{formatBytes(c.budgetBytes)}</bdi></strong><div className="library-usage" role="progressbar" aria-label={t('libraryUsage')} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Number(percent.toFixed(2))}><span style={{width:`${percent}%`}}/></div><dl><div><dt>{t('cacheUsed')}</dt><dd><bdi>{formatBytes(used)}</bdi></dd></div><div><dt>{t('configAvailableCapacity')}</dt><dd><bdi>{formatBytes(Math.max(0,c.budgetBytes-used))}</bdi></dd></div></dl></div>
    </div>
    <div className="configuration-card-footer"><p><Gamepad2 size={17}/><span><bdi>{c.activeCount||0}</bdi> {t('configActiveItems')}</span></p><div className="configuration-card-actions"><button className="secondary" disabled={busy} onClick={event=>{editorTrigger.current=event.currentTarget;edit(c)}}><Settings2 size={16}/>{t('configEdit')}</button><button className="text-button" disabled={busy||!c.activeCount} onClick={()=>void operate({operation:'stop',id:c.id})}><Square size={15}/>{t('configStop')}</button><span className="configuration-action-divider" aria-hidden="true"/><button className="icon-button configuration-delete" aria-label={t('configRemove')} title={t('configRemove')} disabled={busy} onClick={()=>{setRemove(c.id);setError('')}}><Trash2 size={17}/></button></div></div>
    {remove===c.id&&<div className="configuration-confirm"><p>{t('configRemoveHint')}</p><button className="secondary" disabled={busy} onClick={()=>void operate({operation:'remove',id:c.id})}>{t('configConfirmRemove')}</button><button className="text-button" disabled={busy} onClick={()=>setRemove(null)}>{t('close')}</button></div>}
   </section>
  })}</div>
  {data&&!configurations.length&&!editor&&<section className="glass library-empty"><HardDrive size={36}/><h2>{t('configEmpty')}</h2><p>{t('configDescription')}</p><button className="primary" onClick={add}>{t('configAdd')}</button></section>}
  <div className="configuration-safety-note"><ShieldCheck size={18}/><p>{t('configSafetyHint')}</p></div>
 </>;
}
