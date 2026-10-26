import { test, expect } from '@playwright/test';
import { appendHistory, difference, emptyHistory, historyKey, loadHistory, selectHistory, serializeHistory, windows, type Sample } from '../../apps/desktop/src/performance-history';
import type { Telemetry } from '../../apps/desktop/src/types';

const now = Date.parse('2026-10-23T10:00:00Z');
const stats: Telemetry = { protocolVersion:10, experimentalAttachEnabled:false, attached:true, paused:false,
  cacheDisabled:false, hits:100, misses:50, ssdReadBytes:1048576, sourceReadBytes:2097152, fillBytes:0,
  evictions:2, errors:0, generation:7, capacityBytes:25e9, sourceLength:0, activeMutations:0,
  pendingReads:0, readBypasses:0, queueLimit:1, maxReadBytes:65536 };
const sample = (end: number, hits: number): Sample => ({ start:end-1500,end,seconds:1.5,hits,misses:hits,
  ssdBytes:hits*1024,sourceBytes:hits*2048,evictions:1 });
function fixtureHistory() {
  let result = emptyHistory();
  for (const [age, hits] of [[20*windows.day,1],[5*windows.day,2],[12*60*60_000,4],[30*60_000,8],[30_000,16]]) {
    result=appendHistory(result,sample(now-age,hits));
  }
  return result;
}

test('time windows use weighted deltas, retain history and leave gaps blank', () => {
  const history=loadHistory(serializeHistory(fixtureHistory()),now);
  for(const [period,hits] of [['live',16],['hour',24],['day',28],['week',30],['month',31]] as const){
    const selected=selectHistory(history,period,now);
    expect(selected.total.hits).toBe(hits);
    expect(selected.total.ssdBytes).toBe(hits*1024);
    expect(selected.end-selected.start).toBe(windows[period]);
  }
  expect(selectHistory(history,'month',now).points.some(p=>p.ssd===null)).toBe(true);
  let merged=appendHistory(emptyHistory(),sample(now-1500,1));
  merged=appendHistory(merged,sample(now,9));
  const total=selectHistory(merged,'hour',now).total;
  expect(total.seconds).toBe(3);
  expect(total.hits).toBe(10);
  expect(loadHistory(serializeHistory(history),now+31*windows.day)).toEqual(emptyHistory());
  expect(loadHistory('{broken',now)).toEqual(emptyHistory());
});

test('resets, generation changes, pauses and missing intervals never create false reads',()=>{
  const after={...stats,hits:102,misses:51,ssdReadBytes:stats.ssdReadBytes+4096};
  expect(difference(stats,after,now-1500,now)?.hits).toBe(2);
  expect(difference(stats,after,now-1500,now)?.ssdBytes).toBe(4096);
  for(const changed of [{generation:8},{hits:0},{misses:0},{sourceReadBytes:0},{ssdReadBytes:0},{evictions:0},{paused:true},{cacheDisabled:true},{attached:false}]){
    expect(difference(stats,{...after,...changed},now-1500,now)).toBeNull();
  }
  expect(difference(stats,after,now-15_000,now)).toBeNull();
  expect(difference(stats,after,now,now-1)).toBeNull();
});

test('period buttons change chart axes and counters; saved history survives reload and stopped cache',async({page})=>{
  await page.clock.install({time:new Date(now)});
  await page.addInitScript(({raw,key,stats})=>{
    localStorage.setItem('superhdd-welcome','1');
    if(!localStorage.getItem(key))localStorage.setItem(key,raw);
    Object.assign(window,{testTelemetry:stats,isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string)=>{
      if(command==='get_snapshot')return {disks:[],drivers:[],serviceState:'Running',collectedAt:'Performance regression fixture'};
      if(command==='get_cache_telemetry')return structuredClone((window as unknown as {testTelemetry:unknown}).testTelemetry);
      if(command==='get_app_library')return {apps:[],configurations:[],budgetBytes:25e9,allocatedBytes:0,available:false,legacyActive:false};
      if(command==='benchmark_status')return {phase:'idle',running:false,canRecover:false,report:null};
      if(command==='get_cache_runtime'||command==='get_folder_runtime')return {backend:'dokanLibrary',phase:'active',canStart:false,canControl:true,canRemove:false,reason:null,nativeVersion:null,sourceVolume:null,cacheUsedBytes:1048576};
      throw new Error(command);
    }}});
  },{raw:serializeHistory(fixtureHistory()),key:historyKey,stats});
  await page.goto('/');
  await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();
  const counter=page.locator('.six-grid .metric').first().locator('strong');
  const ranges=page.getByRole('group',{name:'Time period',exact:true});
  for(const [period,hits,label] of [['Live',16,'Rolling window (last 2 minutes)'],['1 Hour',24,'Selected period'],['1 Day',28,'Selected period'],['1 Week',30,'Selected period'],['1 Month',31,'Selected period']] as const){
    await ranges.getByRole('button',{name:period,exact:true}).click();
    await expect(counter).toHaveText(String(hits));
    await expect(page.getByTestId('performance-window')).toContainText(label);
    await expect(ranges.getByRole('button',{name:period,exact:true})).toHaveAttribute('aria-pressed','true');
  }
  await expect(page.locator('.chart-card').first()).toContainText('Sep');
  await expect(page.locator('.chart-card').first().locator('.section-heading strong')).toHaveText('50.0%');
  await expect(page.locator('.six-grid .metric').nth(4)).toContainText('Current value');
  await page.screenshot({path:'output/playwright/performance-period-month-en.png',animations:'disabled'});
  await ranges.getByRole('button',{name:'1 Hour',exact:true}).click();
  await expect(page.locator('.chart-card').first()).not.toContainText('Sep');
  await page.screenshot({path:'output/playwright/performance-period-hour-en.png',animations:'disabled'});
  // Collection continues on other pages and records deltas, not lifetime counters.
  await page.locator('nav').getByRole('button',{name:'Storage',exact:true}).click();
  await page.clock.runFor(1600);
  await page.evaluate(()=>{const t=(window as unknown as {testTelemetry:{hits:number;ssdReadBytes:number}}).testTelemetry;t.hits+=10;t.ssdReadBytes+=65536});
  await page.clock.runFor(16_000);
  await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();
  await ranges.getByRole('button',{name:'1 Hour',exact:true}).click();
  await expect(counter).toHaveText('34');
  const saved=await page.evaluate(key=>localStorage.getItem(key),historyKey);
  expect(selectHistory(loadHistory(saved,now+60_000),'hour',now+60_000).total.hits).toBe(34);
  await page.reload();
  await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();
  await ranges.getByRole('button',{name:'1 Hour',exact:true}).click();
  await expect(counter).toHaveText('34');
  await page.evaluate(()=>{(window as unknown as {testTelemetry:{attached:boolean}}).testTelemetry.attached=false});
  await page.clock.runFor(1600);
  await expect(counter).toHaveText('34');
  await expect(page.locator('.benchmark-notice')).toContainText('Previously recorded periods remain available');
  await page.getByRole('button',{name:'العربية',exact:true}).click();
  await page.getByRole('group',{name:'الفترة الزمنية',exact:true}).getByRole('button',{name:'شهر',exact:true}).click();
  await expect(page.locator('html')).toHaveAttribute('dir','rtl');
  expect(await page.locator('main').evaluate(el=>el.scrollWidth>el.clientWidth+1)).toBe(false);
  await page.screenshot({path:'output/playwright/performance-period-month-ar.png',animations:'disabled'});
});
