const {chromium}=require('@playwright/test');
const fs=require('node:fs');
const http=require('node:http');
const crypto=require('node:crypto');
const assert=require('node:assert/strict');
const root=process.cwd();
const stamp=new Date().toISOString().slice(0,10).replaceAll('-','');
const output=`artifacts/runtime/native-audit-final-${stamp}.json`;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function poll(check,timeout=20000){const deadline=Date.now()+timeout;while(Date.now()<deadline){const result=await check();if(result)return result;await sleep(100);}throw Error('Native check timed out');}
(async()=>{
  const browser=await chromium.connectOverCDP('http://127.0.0.1:9223');
  const page=browser.contexts()[0].pages()[0];
  const pageErrors=[];page.on('pageerror',error=>pageErrors.push(error.message));
  page.setDefaultTimeout(20000);
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  const original=await page.evaluate(()=>({language:localStorage.getItem('superhdd-language'),welcome:localStorage.getItem('superhdd-welcome'),destination:localStorage.getItem('superhdd-destination')}));
  await page.evaluate(()=>{localStorage.setItem('superhdd-language','en');localStorage.setItem('superhdd-welcome','1');});
  await page.reload();
  const checks={};
  const runtime=await invoke('get_cache_runtime');
  assert.equal(runtime.reason,'runtimeReleasePending');
  assert.equal(runtime.canStart,false);assert.equal(runtime.canControl,false);assert.equal(runtime.canRemove,false);
  checks.nativeV5ReleaseGate=true;
  await assert.rejects(invoke('cache_operation',{command:{operation:'pause',request:null}}),/runtimeReleasePending/);
  checks.nativeV5PrivilegedActionRefused=true;
  const snapshot=await invoke('get_snapshot',{force:true});
  fs.writeFileSync(`artifacts/runtime/native-snapshot-${stamp}.json`,JSON.stringify(snapshot,null,2));
  assert(!Object.hasOwn(snapshot,'primoPath'));checks.noVendorDiscoveryField=true;
  const source=snapshot.disks.find(d=>d.volumes.some(v=>v.letter==='D:'));
  const ssd=snapshot.disks.find(d=>d.volumes.some(v=>v.letter==='M:'));
  assert.equal(source.kind,'HDD');assert.equal(ssd.kind,'SSD');checks.nativeDiscoveryD=true;
  await page.locator('nav').getByRole('button',{name:'Acceleration',exact:true}).click();
  const sourceSelect=page.getByRole('combobox',{name:'Source HDD'});
  const cacheSelect=page.getByRole('combobox',{name:'Cache SSD'});
  await sourceSelect.selectOption(`${source.id}|D:`);await cacheSelect.selectOption(`${ssd.id}|M:`);
  assert.equal(await page.locator('.hardware.nvme').count(),1);checks.nativeNvme=true;
  await page.getByRole('spinbutton',{name:'Cache size',exact:true}).fill('25');
  const cacheRequest={sourceDiskId:source.id,sourceLetter:'D:',ssdDiskId:ssd.id,ssdLetter:'M:',capacityBytes:24_999_952_384};
  const plan=await invoke('plan_cache',{request:cacheRequest});
  assert.deepEqual(plan.request,cacheRequest);
  assert(plan.remainingBytes>=plan.reserveBytes);checks.native25GBPlanAccepted=true;
  await page.getByRole('button',{name:'Review acceleration',exact:true}).click();
  await page.getByRole('dialog').waitFor();
  assert.match(await page.getByRole('dialog').innerText(),/25\.0 GB/);
  assert.match(await page.getByRole('dialog').innerText(),/Activation unavailable/);checks.nativeReadinessGate=true;
  for(let index=0;index<8;index++){
    await page.keyboard.press('Tab');
    assert(await page.getByRole('dialog').evaluate(dialog=>dialog.contains(document.activeElement)));
  }
  await page.keyboard.press('Escape');
  assert.equal(await page.getByRole('dialog').count(),0);checks.nativeDialogKeyboard=true;
  await page.screenshot({path:'output/playwright/native-setup.png',animations:'disabled'});
  await page.locator('nav').getByRole('button',{name:'Performance',exact:true}).click();
  await page.getByRole('heading',{name:'Hardware file-cache benchmark',exact:true}).waitFor();
  assert.equal(await page.locator('.benchmark-table tbody tr').count(),4);checks.nativePackagedBenchmark=true;
  await page.screenshot({path:'output/playwright/native-benchmark.png',animations:'disabled'});
  await page.locator('.nav-tools summary').click();
  await page.locator('nav').getByRole('button',{name:'Advanced',exact:true}).click();
  assert(!/PrimoCache/.test(await page.locator('main').innerText()));checks.noPrimoCacheUi=true;
  await page.locator('nav').getByRole('button',{name:'Gaming',exact:true}).click();
  assert(await page.getByRole('switch').isDisabled());checks.unsupportedGamingClearlyDisabled=true;
  await page.locator('nav').getByRole('button',{name:'Downloads',exact:true}).click();
  assert.match(await page.locator('main').innerText(),/does not capture browser downloads/);
  await page.getByRole('spinbutton',{name:'SSD staging limit',exact:true}).fill('1');
  const destination='E:/SuperHDD-Native-'+crypto.randomUUID();
  fs.mkdirSync(destination);
  const initialStage=await invoke('get_download_settings');
  const c=snapshot.disks.flatMap(d=>d.volumes).find(v=>v.letter==='C:');
  if(initialStage.root.startsWith('\\\\?\\C:')&&c.free-Math.max(c.size/10,5*1024**3)<1024**3){
    await assert.rejects(invoke('create_download',{url:'http://127.0.0.1:9/reserve.bin',destination,quota:1024**3}),/reserve/);
    checks.nativeStagingReserveGate=true;
  }
  const staging=root+'/artifacts/runtime/download-staging';fs.mkdirSync(staging,{recursive:true});
  const stagingInput=page.getByRole('textbox',{name:'SSD staging folder',exact:true});
  await poll(async()=>!!(await stagingInput.inputValue()));
  await stagingInput.fill(staging);
  await page.getByRole('button',{name:'Use this SSD folder',exact:true}).click();
  await poll(async()=>(await invoke('get_download_settings')).root.includes('download-staging'));
  checks.nativeStagingSwitch=true;
  const payload=Buffer.alloc(2*1024*1024);for(let i=0;i<payload.length;i++)payload[i]=(i*31+Math.floor(i/97))%251;
  let resumed=false;
  const server=http.createServer((request,response)=>{
    const match=/bytes=(\d+)-/.exec(request.headers.range||'');const offset=match?Number(match[1]):0;
    if(offset)resumed=true;
    response.writeHead(offset?206:200,{'Content-Length':payload.length-offset,'ETag':'"native-v1"',...(offset?{'Content-Range':`bytes ${offset}-${payload.length-1}/${payload.length}`}:{})});
    let cursor=offset;
    const timer=setInterval(()=>{if(response.destroyed||cursor>=payload.length){clearInterval(timer);response.end();return;}response.write(payload.subarray(cursor,cursor+16384));cursor+=16384;},5);
    response.on('close',()=>clearInterval(timer));
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  let job;
  try{
    await page.getByRole('button',{name:'New Download',exact:true}).click();
    const dialog=page.getByRole('dialog');
    await dialog.getByLabel('File URL',{exact:true}).fill(`http://127.0.0.1:${server.address().port}/native-probe.bin`);
    await dialog.getByLabel('HDD destination folder',{exact:true}).fill(destination);
    await dialog.getByRole('button',{name:'Start download',exact:true}).click();
    job=await poll(async()=>{const jobs=await invoke('list_downloads');return jobs.find(j=>j.name==='native-probe.bin');});
    await poll(async()=>{const state=(await invoke('list_downloads')).find(j=>j.id===job.id);return state.bytes>=65536;});
    await invoke('download_action',{id:job.id,action:'pause'});
    await poll(async()=>(await invoke('list_downloads')).find(j=>j.id===job.id).state==='paused');
    const actualStage=(await invoke('get_download_settings')).root;
    assert(actualStage.startsWith('\\\\?\\M:'));assert(fs.statSync(require('node:path').join(actualStage,job.id+'.part')).size>0);
    checks.nativeDownloadStageOnM=true;
    await poll(async()=>{try{await invoke('download_action',{id:job.id,action:'resume'});return true;}catch{return false;}});
    const completed=await poll(async()=>{const state=(await invoke('list_downloads')).find(j=>j.id===job.id);if(state.state==='failed')throw Error(state.error);return state.state==='completed'?state:false;});
    assert(resumed);assert(fs.readFileSync(destination+'/native-probe.bin').equals(payload));
    assert.equal(completed.sha256,crypto.createHash('sha256').update(payload).digest('hex'));
    checks.nativeDownloadUi=true;checks.nativeDownloadPauseResume=true;checks.nativeDownloadToEHash=true;
    await page.getByRole('button',{name:'Completed',exact:true}).click();
    const row=page.locator('.download-row').filter({hasText:'native-probe.bin'});
    await row.getByRole('button',{name:'Remove from list',exact:true}).click();
    await poll(async()=>!(await invoke('list_downloads')).some(j=>j.id===job.id));
    assert(fs.readFileSync(destination+'/native-probe.bin').equals(payload));
    checks.nativeRemovePreservesPublishedFile=true;
    await page.screenshot({path:'output/playwright/native-download.png',animations:'disabled'});
  }finally{server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
  await page.evaluate(()=>localStorage.setItem('superhdd-language','ar'));await page.reload();
  await page.locator('nav').getByRole('button',{name:'الأداء',exact:true}).click();
  await page.getByRole('heading',{name:'بنشمارك كاش الملفات على الأقراص',exact:true}).waitFor();
  await page.screenshot({path:'output/playwright/native-benchmark-ar.png',animations:'disabled'});checks.nativeArabic=true;
  await page.evaluate(original=>{
    for(const [key,value] of [['superhdd-language',original.language],['superhdd-welcome',original.welcome],['superhdd-destination',original.destination]]){
      if(value===null)localStorage.removeItem(key);else localStorage.setItem(key,value);
    }
  },original);
  assert.equal(crypto.createHash('sha256').update(fs.readFileSync(destination+'/native-probe.bin')).digest('hex'),crypto.createHash('sha256').update(payload).digest('hex'));
  fs.unlinkSync(destination+'/native-probe.bin');fs.rmdirSync(destination);
  checks.nativeTestPayloadsCleaned=true;
  if((await invoke('get_download_settings')).root!==initialStage.root){
    const restored=await invoke('set_download_staging',{root:initialStage.root});
    assert.equal(restored.root,initialStage.root);
  }
  checks.nativeStagingChoicePreserved=true;
  await page.reload();
  assert.deepEqual(pageErrors,[]);checks.nativeNoScriptErrors=true;
  fs.writeFileSync(output,JSON.stringify({at:new Date().toISOString(),scope:'Actual release WebView2 application with real native Tauri commands; D/M cache plan and disposable E/M download check',checks,cachePlan:plan,jobId:job?.id,downloadDestination:destination,downloadStaging:(await invoke('get_download_settings')).root,sourceMediaEvidence:source.mediaEvidence},null,2));
  console.log(fs.readFileSync(output,'utf8'));
  await browser.close();
})().catch(error=>{console.error(error);process.exit(1);});
