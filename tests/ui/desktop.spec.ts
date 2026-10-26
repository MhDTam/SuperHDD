import { test, expect, type Page } from "@playwright/test";

async function runtimeFixture(page:Page,initialPhase:string,reason:string|null=null,cancelStart=false,nativeVersion=5,capacityBytes=24_999_952_384,folderBackend=false){
 await page.addInitScript(({initialPhase,reason,cancelStart,nativeVersion,capacityBytes,folderBackend})=>{
  localStorage.setItem('superhdd-welcome','1');
  localStorage.setItem('superhdd-folder-workflow-v1','1');
  if(!localStorage.getItem('superhdd-scope'))localStorage.setItem('superhdd-scope',folderBackend?'selected':'volume');
  const disk=(number:number,letter:string,kind:string)=>({number,id:`disk-${number}`,model:`Test ${kind}`,serial:`serial-${number}`,kind,bus:'SATA',size:500*1024**3,online:true,readOnly:false,system:false,volumes:[{letter,label:'Test',fileSystem:'NTFS',size:500*1024**3,free:200*1024**3,partitionNumber:1,system:false,health:'Healthy',encryption:'FullyDecrypted'}]});
  let phase=initialPhase;let installed=initialPhase!=='stopped';
  const state=()=>({phase,canStart:!reason&&phase==='stopped',canControl:!reason&&['active','paused','faulted'].includes(phase),canRemove:!reason&&installed,reason,nativeVersion,sourceVolume:nativeVersion>=6?'D:':null,...(folderBackend?{backend:'dokanFolder',cacheUsedBytes:phase==='active'?4096:0,cachedFiles:phase==='active'?[{path:'D:\\Games\\Example\\assets.pak',residentBytes:4096}]:[]} :{}),...(nativeVersion>=6&&phase!=='stopped'?{request:{sourceDiskId:'disk-1',sourceLetter:'D:',ssdDiskId:'disk-2',ssdLetter:'M:',capacityBytes,...(folderBackend?{scope:{mode:'selected',folders:['D:\\Games\\Example']}}:{})}}:{})});
  const calls:{operation:string;request:unknown}[]=[];
  Object.assign(window,{runtimeCalls:calls,isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string,args?:{command?:{operation:string;request:unknown};request?:unknown})=>{
   if(command==='get_snapshot'){const source=disk(1,'D:','HDD');if(nativeVersion===6&&phase!=='stopped'){source.online=false;source.volumes=[]}return {disks:[source,disk(2,'M:','SSD'),disk(3,'E:','HDD')],drivers:[],serviceState:installed?'Running':'NotInstalled',collectedAt:'Mocked runtime UI test'};}
   if(command==='get_cache_runtime')return state();
   if(command==='get_folder_runtime')return folderBackend?state():{phase:'stopped',canStart:false,canControl:false,canRemove:false,backend:'dokanFolder',reason:'folderRuntimeUnavailable'};
   if(command==='get_cache_telemetry')return null;
   if(command==='plan_cache')return {request:args?.request,reserveBytes:50*1024**3,remainingBytes:180*1024**3,recommendedBytes:20*1024**3,activationAvailable:!reason,reasons:reason?[reason]:[]};
   if(command==='cache_operation' || command==='folder_operation'){
    if(folderBackend && command!=='folder_operation')throw Error('Folder request routed to volume backend');
    if(!folderBackend && command==='folder_operation')throw Error('Volume request routed to folder backend');
    const request=args!.command!;calls.push(request);
    if(request.operation==='start'&&cancelStart)throw 'runtimeCancelled';
    if(request.operation==='start'){phase='active';installed=true}
    if(request.operation==='pause')phase='paused';
    if(request.operation==='resume')phase='active';
    if(request.operation==='stop')phase='stopped';
    if(request.operation==='remove')installed=false;
    return state();
   }
   throw Error('Unexpected v5 mock command '+command);
  }}});
 },{initialPhase,reason,cancelStart,nativeVersion,capacityBytes,folderBackend});
 await page.goto('/');
 await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();
}
test('native cache keeps the actual D/M allocation visible while its physical source is offline',async({page})=>{
 await runtimeFixture(page,'active',null,false,6);
 await expect(page.getByRole('status')).toHaveText('Running');
 await expect(page.locator('.runtime-session')).toContainText('D:');
 await expect(page.locator('.runtime-session')).toContainText('M:');
 await expect(page.locator('.runtime-session')).toContainText('25 GB');
 await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'Pause',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Paused');
 await page.getByRole('button',{name:'Clear cached copies',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Paused');
 await page.getByRole('button',{name:'Resume',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Running');
 await page.screenshot({path:'output/playwright/native-v6-allocation.png',animations:'disabled'});
 await page.getByRole('button',{name:'Stop cache',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Stopped');
 await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toBeVisible();
});
for(const version of [7,8]){
test(`v${version} filter shows the actual saved allocation while its source remains mounted`,async({page})=>{
 await runtimeFixture(page,'active',null,false,version,7_999_979_520);
 await expect(page.locator('.runtime-session')).toContainText('D:');
 await expect(page.locator('.runtime-session')).toContainText('M:');
 await expect(page.locator('.runtime-session')).toContainText('8 GB');
 await expect(page.locator('.runtime-session')).not.toContainText('25 GB');
 await page.getByRole('button',{name:'Pause',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Paused');
 await page.getByRole('button',{name:'Resume',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Running');
});

test(`v${version} missing-driver reason blocks activation without a Windows permission request`,async({page})=>{
 await runtimeFixture(page,'stopped','runtimeKernelDriverUnavailable',false,version);
 await page.getByRole('button',{name:'Review acceleration',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('The Windows storage cache driver is not loaded.');
 await expect(page.getByRole('button',{name:'Start cache',exact:true})).toHaveCount(0);
 expect(await page.evaluate(()=>(window as unknown as {runtimeCalls:unknown[]}).runtimeCalls)).toEqual([]);
});
}

test('service controls track pause, resume, clear, stop and removal through native IPC',async({page})=>{
 await runtimeFixture(page,'active');
 await page.getByRole('button',{name:'Pause',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Paused');
 await page.getByRole('button',{name:'Clear cached copies',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Paused');
 await page.getByRole('button',{name:'Resume',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Running');
 await page.getByRole('button',{name:'Stop cache',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Stopped');
 await page.getByRole('button',{name:'Remove cache setup',exact:true}).click();
 await expect(page.getByRole('button',{name:'Remove cache setup',exact:true})).toHaveCount(0);
 const calls=await page.evaluate(()=>(window as unknown as {runtimeCalls:{operation:string;request:unknown}[]}).runtimeCalls);
 expect(calls).toEqual(['pause','clear','resume','stop','remove'].map(operation=>({operation,request:null})));
});
test('cancelled Windows permission preserves stopped state and the reviewed source request',async({page})=>{
 await runtimeFixture(page,'stopped',null,true);
 await page.getByRole('button',{name:'Review acceleration',exact:true}).click();
 await page.getByRole('dialog').getByRole('button',{name:'Start cache',exact:true}).click();
 await expect(page.getByRole('dialog').getByRole('alert')).toContainText('Windows permission request was cancelled.');
 await page.keyboard.press('Escape');
 await expect(page.getByRole('dialog')).toHaveCount(0);
 await expect(page.getByRole('status')).toHaveText('Stopped');
 await expect(page.getByRole('button',{name:'Stop cache',exact:true})).toHaveCount(0);
 const calls=await page.evaluate(()=>(window as unknown as {runtimeCalls:unknown[]}).runtimeCalls);
 expect(calls).toEqual([{operation:'start',request:{sourceDiskId:'disk-1',sourceLetter:'D:',ssdDiskId:'disk-2',ssdLetter:'M:',capacityBytes:24_999_952_384}}]);
});
test('unqualified package exposes its reason and cannot request activation',async({page})=>{
 await runtimeFixture(page,'stopped','runtimeReleasePending');
 await page.getByRole('button',{name:'Review acceleration',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('The deployable driver package has not been signed and qualified yet.');
 await expect(page.getByRole('button',{name:'Start cache',exact:true})).toHaveCount(0);
 expect(await page.evaluate(()=>(window as unknown as {runtimeCalls:unknown[]}).runtimeCalls)).toEqual([]);
});

test('selected folders persist per source and cannot start a whole-volume engine', async ({page}) => {
 await runtimeFixture(page, 'stopped');
 await page.getByRole('button', {name:'Selected games and programs only',exact:false}).click();
 const review=page.getByRole('button',{name:'Review acceleration',exact:true});
 await expect(review).toBeDisabled();
 const folder=page.getByRole('textbox',{name:'Game or program folder on the selected HDD'});
 for(const path of ['M:\\Games\\Example', 'D:\\', 'D:\\Games\\..\\Other', 'D:\\Games\\data:stream']) {
  await folder.fill(path);await page.getByRole('button',{name:'Add folder',exact:true}).click();
  await expect(page.getByRole('alert')).toContainText('Enter a full subfolder path');
  await expect(review).toBeDisabled();
 }
 await folder.fill('D:/Games/Example/');await folder.press('Enter');
 await expect(page.getByRole('list',{name:'Selected folders'})).toContainText('D:\\Games\\Example');
 await folder.fill('d:\\games\\example');await folder.press('Enter');
 await expect(page.getByRole('alert')).toContainText('already selected');
 await page.getByRole('combobox',{name:'Source HDD'}).selectOption('disk-3|E:');
 await expect(page.getByRole('list',{name:'Selected folders'})).toHaveCount(0);
 await expect(review).toBeDisabled();
 await page.getByRole('combobox',{name:'Source HDD'}).selectOption('disk-1|D:');
 await page.reload();await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();
 await expect(page.getByRole('list',{name:'Selected folders'})).toContainText('D:\\Games\\Example');
 await review.click();
 await expect(page.getByRole('dialog')).toContainText('D:\\Games\\Example');
 await expect(page.getByRole('dialog')).toContainText('Selected-folder caching is not supported');
 // The fixture deliberately claims activationAvailable=true: the UI must still refuse it.
 await expect(page.getByRole('button',{name:'Start cache',exact:true})).toHaveCount(0);
 expect(await page.evaluate(()=>(window as unknown as {runtimeCalls:unknown[]}).runtimeCalls)).toEqual([]);
 await page.keyboard.press('Escape');
 await page.getByRole('button',{name:'Remove folder D:\\Games\\Example',exact:true}).click();
 await expect(review).toBeDisabled();
});

test('qualified folder backend starts selected scope and displays resident file data',async({page})=>{
 await runtimeFixture(page,'stopped',null,false,9,24_999_952_384,true);
 await page.getByRole('textbox',{name:'Game or program folder on the selected HDD'}).fill('D:\\Games\\Example');
 await page.getByRole('button',{name:'Add folder',exact:true}).click();
 await page.getByRole('button',{name:'Review acceleration',exact:true}).click();
 await expect(page.getByRole('dialog')).toContainText('Close the game before starting or stopping');
 await page.getByRole('button',{name:'Start cache',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Running');
 await expect(page.getByTestId('cache-contents')).toContainText('assets.pak');
 await expect(page.getByTestId('cache-contents')).not.toContainText('does not report cached file names');
 const calls=await page.evaluate(()=>(window as unknown as {runtimeCalls:{operation:string;request:{scope?:unknown}}[]}).runtimeCalls);
 expect(calls[0].request.scope).toEqual({mode:'selected',folders:['D:\\Games\\Example']});
 await page.getByRole('button',{name:'Stop cache',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Stopped');
});

test('active folder backend restores its selected path when the desktop opens',async({page})=>{
 await runtimeFixture(page,'active',null,false,9,24_999_952_384,true);
 await expect(page.locator('.selected-folders').first()).toContainText('D:\\Games\\Example');
 await expect(page.getByRole('status')).toHaveText('Running');
 await expect(page.getByTestId('cache-contents')).toContainText('assets.pak');
 await page.getByRole('button',{name:'Stop cache',exact:true}).click();
 await expect(page.getByRole('status')).toHaveText('Stopped');
 await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toBeEnabled();
 const saved=await page.evaluate(()=>JSON.parse(localStorage.getItem('superhdd-selected-folders')||'{}'));
 expect(saved['disk-1|D:']).toEqual(['D:\\Games\\Example']);
});

test('cache contents never confuse requested capacity or cumulative fills with occupancy', async ({page}) => {
 await runtimeFixture(page,'active');
 const contents=page.getByTestId('cache-contents');
 await expect(contents).toContainText('does not report cached file names or live occupied bytes');
 await expect(contents).toContainText('Not available');
 await expect(contents).not.toContainText('25.0');
 await contents.getByRole('button',{name:'Clear cached copies',exact:true}).click();
 const calls=await page.evaluate(()=>(window as unknown as {runtimeCalls:{operation:string}[]}).runtimeCalls);
 expect(calls.map(call=>call.operation)).toEqual(['clear']);
 await expect(contents).toContainText('the original HDD files are preserved');
});

test('Arabic selective setup remains usable with long folder paths', async ({page}) => {
 await runtimeFixture(page,'stopped');
 await page.getByRole('button',{name:'العربية',exact:true}).click();
 await page.getByRole('button',{name:'ألعاب وبرامج محددة فقط',exact:false}).click();
 const path='D:\\Games\\'+ 'LongGameFolder'.repeat(12);
 await page.getByRole('textbox',{name:'مجلد اللعبة أو البرنامج على HDD المحدد'}).fill(path);
 await page.getByRole('button',{name:'إضافة مجلد',exact:true}).click();
 for(const width of [1440,1050]){
  await page.setViewportSize({width,height:940});
  expect(await page.locator('main').evaluate(el=>el.scrollWidth>el.clientWidth+1)).toBe(false);
  await expect(page.locator('.sidebar')).toBeInViewport({ratio:1});
  expect(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1)).toBe(false);
 }
 await page.locator('main').evaluate(el=>el.scrollTo(0,el.scrollHeight));
 await page.screenshot({path:'output/playwright/selective-cache-ar.png',animations:'disabled'});
});
test('native v4 volume telemetry is visible without enabling production acceleration',async({page})=>{
 await page.addInitScript(()=>{
  localStorage.setItem('superhdd-welcome','1');
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string)=>{
   if(command==='get_snapshot')return {disks:[],drivers:[{name:'SuperHDDRawLab',state:'Running'}],serviceState:'Running',collectedAt:'Mocked native-v4 UI regression'};
   if(command==='get_cache_telemetry')return {protocolVersion:4,experimentalAttachEnabled:true,attached:true,paused:false,cacheDisabled:false,hits:10,misses:4,sourceReadBytes:262144,ssdReadBytes:655360,fillBytes:262144,evictions:0,errors:0,generation:3,capacityBytes:65536,sourceLength:131072,activeMutations:0,pendingReads:0,readBypasses:9,queueLimit:16,maxReadBytes:1048576,nativeVolume:{volumeCount:1,sourceDiskNumber:2,backingDiskNumber:3,pnpState:1,storeLocked:true,allocatedBytes:73728,cancelledReads:7,childCancellations:0,pagingHits:22,revalidatedRecords:3,validationReadBytes:196608}};
   throw Error('Unexpected native-v4 mock command '+command);
  }}});
 });
 await page.goto('/');
 await expect(page.locator('main')).toContainText('unqualified disposable-VHD lab build');
 await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();await page.locator('.performance-diagnostics>summary').click();
 await expect(page.getByTestId('engine-diagnostics')).toContainText('Driver protocol: 4');
 const native=page.getByTestId('native-volume-diagnostics');
 await expect(native).toContainText('Source disk number: 2');
 await expect(native).toContainText('Cache disk number: 3');
 await expect(native).toContainText('Cancelled cache reads: 7');
 await expect(native).toContainText('Paging reads served from SSD: 22');
 await expect(page.getByRole('button',{name:'Pause',exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'العربية',exact:true}).click();
 await expect(page.locator('html')).toHaveAttribute('dir','rtl');
 await expect(native).toContainText('قراءات الكاش الملغاة: 7');
 expect(await page.locator('main').evaluate(element=>element.scrollWidth>element.clientWidth+1)).toBe(false);
 await page.screenshot({path:'output/playwright/native-v4-rtl.png',animations:'disabled'});
});
for(const state of ['paused','bypass'] as const){
test(`mocked driver v2 ${state} status is explicit and does not invent active throughput`,async({page})=>{
 await page.addInitScript(({state})=>{
  localStorage.setItem('superhdd-welcome','1');
  Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string)=>{
   if(command==='get_snapshot')return {disks:[],drivers:[{name:'SuperHDDCacheDriver',state:'Running'}],serviceState:'Running',primoPath:null,collectedAt:'Mocked driver protocol UI test'};
   if(command==='get_cache_telemetry')return {protocolVersion:2,experimentalAttachEnabled:true,attached:true,paused:state==='paused',cacheDisabled:state==='bypass',hits:10,misses:4,sourceReadBytes:262144,ssdReadBytes:655360,fillBytes:262144,evictions:0,errors:0,generation:3,capacityBytes:65536,sourceLength:131072,activeMutations:0,pendingReads:0,readBypasses:9,queueLimit:16,maxReadBytes:1048576};
   throw Error('Unexpected mocked IPC command '+command);
  }}});
 },{state});
 await page.goto('/');
 await expect(page.locator('.status-pill')).toHaveText(state==='paused'?'Cache paused':'Cache bypass after device transition');
 await expect(page.locator('main')).toContainText('unqualified disposable-VHD lab build');
 await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();await page.locator('.performance-diagnostics>summary').click();
 await expect(page.getByTestId('engine-diagnostics')).toContainText('Driver protocol: 2');
 await expect(page.getByTestId('engine-diagnostics')).toContainText('0 / 16');
 await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();
 await expect(page.locator('.recharts-wrapper')).toHaveCount(0);
});
}
test('review dialog keeps keyboard focus and restores its opener',async({page})=>{
 await runtimeFixture(page,'stopped');
 const opener=page.getByRole('button',{name:'Review acceleration',exact:true});await opener.click();
 const dialog=page.getByRole('dialog');await expect(dialog).toBeFocused();
 const closeButtons=dialog.getByRole('button',{name:'Close',exact:true});
 await page.keyboard.press('Shift+Tab');await expect(closeButtons.last()).toBeFocused();
 await page.keyboard.press('Tab');await expect(closeButtons.first()).toBeFocused();
 await page.keyboard.press('Escape');await expect(dialog).toHaveCount(0);await expect(opener).toBeFocused();
 await expect(page.locator('main')).not.toHaveAttribute('inert','');
});

test('high DPI layout and keyboard numeric entry stay usable',async({page,browser})=>{
 const context=await browser.newContext({viewport:{width:1050,height:760},deviceScaleFactor:2});const hi=await context.newPage();await hi.goto('/?demo=1');await hi.getByRole('button',{name:'Get Started'}).click();
 const capacity=hi.getByRole('spinbutton',{name:'Cache size',exact:true});await capacity.fill('');await capacity.pressSequentially('125');await expect(capacity).toHaveValue('125');await expect(capacity).toBeFocused();
 const overflow=await hi.locator('main').evaluate(element=>element.scrollWidth>element.clientWidth+1);expect(overflow).toBe(false);
 await hi.screenshot({path:'output/playwright/high-dpi.png',animations:'disabled'});await context.close();
});
test("setup defaults to D, M and 25 GB and preserves later selections", async ({page}) => {
  const makeDisk=(number:number,letter:string,kind:string,bus:string,model:string)=>({number,id:`disk-${number}`,model,serial:"TEST",kind,bus,size:500*1024**3,online:true,readOnly:false,system:letter==="E:",mediaEvidence:"Test fixture: platform-classified media",volumes:[{letter,label:"Test",fileSystem:"NTFS",size:500*1024**3,free:200*1024**3,partitionNumber:2,system:false,health:"Healthy",encryption:"FullyDecrypted"}]});
  const snapshot={disks:[makeDisk(0,"F:","SSD","SATA","Test SATA SSD"),makeDisk(1,"E:","HDD","SATA","WDC WD10EURX-63FH1Y0"),makeDisk(2,"D:","HDD","SATA","Test HDD"),makeDisk(3,"M:","SSD","NVMe","Samsung SSD 980 PRO 1TB")],drivers:[],serviceState:"NotInstalled",primoPath:"test-reference",collectedAt:"2026-10-18T00:00:00Z"};
  await page.addInitScript((data)=>{
    localStorage.setItem("superhdd-welcome","1");
    Object.assign(window,{isTauri:true,__TAURI_INTERNALS__:{invoke:async(command:string)=>{
      if(command==="get_snapshot")return data;
      if(command==="get_cache_telemetry")return null;
      throw new Error("Unsupported test command");
    }}});
  },snapshot);
  await page.goto("/");
  await expect(page.locator("nav > button")).toHaveCount(6);
  await expect(page.locator(".nav-tools")).toHaveCount(0);
  await expect(page.locator(".four-grid")).toHaveCount(0);
  await page.locator("nav").getByRole("button",{name:"Cache settings",exact:true}).click();
  await expect(page.getByRole("combobox",{name:"Source HDD"})).toHaveValue("disk-2|D:");
  await expect(page.getByRole("combobox",{name:"Cache SSD"})).toHaveValue("disk-3|M:");
  await expect(page.getByRole("spinbutton",{name:"Cache size",exact:true})).toHaveValue("25");
  await expect(page.getByRole("button",{name:"Review acceleration",exact:true})).toBeDisabled();
  await page.getByRole('textbox',{name:'Game or program folder on the selected HDD'}).fill('D:\\Games\\Example');
  await page.getByRole('button',{name:'Add folder',exact:true}).click();
  await expect(page.getByRole("button",{name:"Review acceleration",exact:true})).toBeEnabled();
  await expect(page.locator(".hardware.nvme")).toBeVisible();
  await page.getByRole("combobox",{name:"Source HDD"}).selectOption("disk-1|E:");
  await page.getByRole("combobox",{name:"Cache SSD"}).selectOption("disk-0|F:");
  await page.getByRole("spinbutton",{name:"Cache size",exact:true}).fill("18.5");
  await page.reload();
  await page.locator("nav").getByRole("button",{name:"Cache settings",exact:true}).click();
  await expect(page.getByRole("combobox",{name:"Source HDD"})).toHaveValue("disk-1|E:");
  await expect(page.getByRole("combobox",{name:"Cache SSD"})).toHaveValue("disk-0|F:");
  await expect(page.getByRole("spinbutton",{name:"Cache size",exact:true})).toHaveValue("18.5");
  await page.screenshot({path:"output/playwright/audit-setup-fixture.png",animations:"disabled"});
  await page.locator("nav").getByRole("button",{name:"Performance & diagnostics",exact:true}).click();await page.locator(".performance-diagnostics>summary").click();
  await expect(page.locator("main")).not.toContainText("PrimoCache");
  await expect(page.locator("nav").getByRole("button",{name:/Downloads|Gaming|Advanced/,exact:true})).toHaveCount(0);
});

test("benchmark runs replace static machine-specific scores", async ({page})=>{
  await page.addInitScript(()=>localStorage.setItem("superhdd-welcome","1"));
  await page.goto("/");
  await page.locator("nav").getByRole("button",{name:"Performance & diagnostics",exact:true}).click();
  await expect(page.getByRole("region",{name:"Storage benchmark",exact:true})).toContainText("not game loading time or FPS");
  await expect(page.locator(".benchmark-results")).toHaveCount(0);
});

test("welcome, manual capacity and honest activation gate", async ({
  page,
}) => {
  await page.goto("/?demo=1");
  await expect(page.locator(".demo-banner")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Make Your HDD Super Fast." }),
  ).toBeVisible();
  await page.screenshot({
    path: "output/playwright/welcome.png",
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Get Started" }).click();
  await page.getByRole('button',{name:'Everything on this HDD volume',exact:false}).click();
  const input = page.getByRole("spinbutton", {
    name: "Cache size",
    exact: true,
  });
  await input.fill("20");
  await expect(input).toHaveValue("20");
  await page.getByRole("button", { name: "Review acceleration" }).click();
  await expect(page.getByRole("dialog")).toContainText(
    "Activation unavailable",
  );
  await expect(page.getByRole("dialog")).toContainText("20.0 GB");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Close", exact: true })
    .last()
    .click();
  await input.fill('100');
  await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toBeDisabled();
  await input.fill('1');
  await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toBeDisabled();
  await input.fill('');
  await expect(page.getByRole('button',{name:'Review acceleration',exact:true})).toBeDisabled();
  await input.fill('20');
  await page.locator("main").evaluate((el) => el.scrollTo(0, 0));
  await page.screenshot({
    path: "output/playwright/acceleration.png",
    animations: "disabled",
  });
});
test("all pages render without script errors or horizontal overflow", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?demo=1");
  await page.getByRole("button", { name: "Get Started" }).click();
  for (const width of [1440, 1366, 1050]) {
    await page.setViewportSize({ width, height: 768 });
    for (const name of [
      "Home",
      "Performance & diagnostics",
      "Library",
      "Cache settings",
      "Storage",
      "Settings",
    ]) {
      await page
        .locator("nav")
        .getByRole("button", { name, exact: true })
        .click();
      await expect(page.locator("main h1")).toBeVisible();
      const overflow = await page
        .locator("main")
        .evaluate((el) => el.scrollWidth > el.clientWidth + 1);
      expect(overflow, `${name} at ${width}px`).toBe(false);
    }
  }
  expect(errors).toEqual([]);
});
test("Arabic mirrors the shell and persists language", async ({ page }) => {
  await page.goto("/?demo=1");
  await page.getByRole("button", { name: "Get Started" }).click();
  await page.getByRole("button", { name: "العربية", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("dir", "rtl");
  await expect(
    page.getByRole("heading", { name: "سرّع قرصك الصلب" }),
  ).toBeVisible();
  await page
    .locator("nav")
    .getByRole("button", { name: "التخزين", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "نظرة على التخزين" }),
  ).toBeVisible();
  await page.screenshot({
    path: "output/playwright/storage-ar.png",
    animations: "disabled",
  });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("lang", "ar");
});
test("ordinary browser preview never invents live hardware", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Get Started" }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Open the Windows desktop application",
  );
  await expect(
    page.getByRole("button", { name: "Review acceleration" }),
  ).toBeDisabled();
  await expect(page.locator("main")).not.toContainText("Seagate");
});
test("preferences work and unavailable features stay disabled", async ({
  page,
}) => {
  await page.goto("/?demo=1");
  await page.getByRole("button", { name: "Get Started" }).click();
  await page
    .locator("nav")
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await expect(
    page.getByRole("switch", { name: "Start with Windows" }),
  ).toBeDisabled();
  await page.getByRole("switch", { name: "Reduce motion" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-motion", "reduced");
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await page.getByRole("combobox", { name: "Theme" }).selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.screenshot({
    path: "output/playwright/settings-dark.png",
    animations: "disabled",
  });
});
