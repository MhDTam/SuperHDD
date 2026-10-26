import {test,expect,type Page} from '@playwright/test';
async function libraryFixture(page:Page,arabic=false,icons=false,scopes=false){
 await page.addInitScript(({arabic,icons,scopes})=>{
  localStorage.setItem('superhdd-welcome','1');localStorage.setItem('superhdd-language',arabic?'ar':'en');
  const apps=[
   {id:'a'.repeat(32),name:'Dragon Ball',path:'D:\\Games\\Dragon Ball',origin:'Steam',kind:'game',iconDataUri:null,managed:true,phase:'active',eligible:true,reason:null,cacheUsedBytes:4000000000,allocationBytes:12499976192,canControl:true,cachedFiles:[{path:'D:\\Games\\Dragon Ball\\assets.pak',residentBytes:3999740000},{path:'D:\\Games\\Dragon Ball\\Data\\Textures.ucas',residentBytes:256000},{path:'D:\\Games\\Dragon Ball\\game.exe',residentBytes:4000}]},
   {id:'b'.repeat(32),name:'Tekken',path:'D:\\Games\\Tekken',origin:'Games folder',kind:'game',iconDataUri:null,managed:true,phase:'active',eligible:true,reason:null,cacheUsedBytes:2000000000,allocationBytes:12499976192,canControl:true,cachedFiles:[{path:'D:\\Games\\Tekken\\assets.pak',residentBytes:2000000000}]},
   {id:'c'.repeat(32),name:'Editor',path:'C:\\Programs\\Editor',origin:'Windows',kind:'app',iconDataUri:null,managed:false,phase:'stopped',eligible:false,reason:'libraryDifferentDrive',cacheUsedBytes:0,allocationBytes:0,canControl:false,cachedFiles:[]}
  ];
  if(icons){apps[0].iconDataUri='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aZ1sAAAAASUVORK5CYII=' as any;apps[1].iconDataUri='data:image/png;base64,broken' as any}
  if(scopes){Object.assign(apps[0],{cacheMode:'full',dataCacheAvailable:true,dataCachePath:'D:\\Games\\Dragon Ball\\Game\\Content\\Paks'});Object.assign(apps[1],{cacheMode:'full',dataCacheAvailable:false,dataCachePath:null})}
  let budgetBytes=24999952384;const calls:unknown[]=[];
  const configurations=[{id:'x'.repeat(32),name:'D to M',sourceDiskId:'source-d',sourceLetter:'D:',ssdDiskId:'ssd-m',ssdLetter:'M:',budgetBytes:24999952384}];
  const disk=(id:string,letter:string,kind:string,system=false)=>({id,number:0,model:'Test '+letter,serial:id,kind,bus:'SATA',size:1e12,online:true,readOnly:false,system,volumes:[{letter,label:letter,fileSystem:'NTFS',size:1e12,free:200e9,partitionNumber:1,system,health:'Healthy',encryption:'FullyDecrypted'}]});
  apps.push({id:'e'.repeat(32),name:'Racer',path:'E:\\Games\\Racer',origin:'Steam',kind:'game',iconDataUri:null,managed:false,phase:'stopped',eligible:false,reason:'configChooseDrive',cacheUsedBytes:0,allocationBytes:0,canControl:false,cachedFiles:[]});

  const state=()=>{for(const c of configurations){const live=apps.filter(a=>a.path.startsWith(c.sourceLetter+'\\')&&a.phase!=='stopped');for(const a of live)a.allocationBytes=0}return {apps:structuredClone(apps).map(a=>{const c=configurations.find(c=>a.path.startsWith(c.sourceLetter+'\\'));return {...a,eligible:!!c,configurationId:c?.id,sourceLetter:c?.sourceLetter,ssdLetter:c?.ssdLetter,sharedPool:!!c,sharedCapacityBytes:c?.budgetBytes,reason:c?null:'configChooseDrive'}}),configurations:configurations.map(c=>({...c,cacheUsedBytes:apps.filter(a=>a.path.startsWith(c.sourceLetter+'\\')&&a.phase!=='stopped').reduce((sum,a)=>sum+a.cacheUsedBytes,0),allocatedBytes:apps.some(a=>a.path.startsWith(c.sourceLetter+'\\')&&a.phase!=='stopped')?c.budgetBytes:0,activeCount:apps.filter(a=>a.path.startsWith(c.sourceLetter+'\\')&&a.phase!=='stopped').length})),cachePolicy:'loadingPriority',cacheUsedBytes:apps.reduce((sum,a)=>sum+a.cacheUsedBytes,0),budgetBytes:configurations.reduce((sum,c)=>sum+c.budgetBytes,0),allocatedBytes:apps.reduce((sum,a)=>sum+a.allocationBytes,0),available:true,legacyActive:false}};

  Object.assign(window,{isTauri:true,libraryCalls:calls,__TAURI_INTERNALS__:{invoke:async(command:string,args?:any)=>{
   if(command==='get_app_library')return state();
   if(command==='benchmark_status')return {phase:'idle',running:false,canRecover:false,report:null};
   if(command==='get_snapshot')return {disks:[disk('source-d','D:','HDD'),disk('source-e','E:','HDD'),disk('ssd-m','M:','SSD'),disk('ssd-f','F:','SSD'),disk('system','C:','SSD',true)],drivers:[],serviceState:'NotInstalled',collectedAt:'fixture'};
   if(command==='get_folder_runtime'||command==='get_cache_runtime')return {backend:'dokanLibrary',phase:'active',canStart:false,canControl:true,canRemove:false,reason:null,nativeVersion:null,sourceVolume:'D:'};
   if(command==='get_cache_telemetry')return null;
   if(command==='app_library_operation'){calls.push(args.command);const c=args.command;const app=apps.find(a=>a.id===c.id);if(c.operation==='modeData'||c.operation==='modeFull')Object.assign(app!,{cacheMode:c.operation==='modeData'?'dataOnly':'full'});if(c.operation==='start'){app!.managed=true;app!.phase='active'}if(c.operation==='clear'){app!.cacheUsedBytes=0;app!.cachedFiles=[]}if(c.operation==='pause')app!.phase='paused';if(c.operation==='resume')app!.phase='active';if(c.operation==='stop'){app!.phase='stopped';app!.allocationBytes=0;app!.cacheUsedBytes=0;app!.cachedFiles=[]}if(c.operation==='budget'){budgetBytes=c.budgetBytes;const live=apps.filter(a=>a.phase!=='stopped');for(const a of live)a.allocationBytes=Math.floor(budgetBytes/live.length/65536)*65536}return state()}
   if(command==='cache_configuration_operation'){const c=args.command;calls.push(structuredClone(c));if(c.operation==='save'){const next={...c.configuration,id:c.configuration.id||'y'.repeat(32),name:c.configuration.name||c.configuration.sourceLetter+' to '+c.configuration.ssdLetter};const index=configurations.findIndex(x=>x.id===next.id);if(index>=0)configurations[index]=next;else configurations.push(next)}else{const config=configurations.find(x=>x.id===c.id)!;for(const a of apps.filter(a=>a.path.startsWith(config.sourceLetter+'\\'))){a.phase='stopped';a.cacheUsedBytes=0;a.allocationBytes=0;a.cachedFiles=[]}if(c.operation==='remove')configurations.splice(configurations.indexOf(config),1)}return state()}
   if(command==='pick_app_folder')return 'D:\\Games\\Collection';
   if(command==='add_library_folder'){calls.push({operation:'add',...args});apps.push({id:'d'.repeat(32),name:args.name||'Collection',path:args.path,origin:'Manual',kind:'app',iconDataUri:null,managed:true,phase:'stopped',eligible:true,reason:null,cacheUsedBytes:0,allocationBytes:0,canControl:false,cachedFiles:[]});return state()}
   throw Error('Unexpected library fixture command: '+command);
  }}});
 },{arabic,icons,scopes});
 await page.goto('/');await page.locator('nav').getByRole('button',{name:arabic?'المكتبة':'Library',exact:true}).click();
}

test('library renders real icons and falls back cleanly for invalid images',async({page})=>{
 await libraryFixture(page,false,true);
 const game=page.getByRole('region',{name:'Dragon Ball',exact:true});
 const icon=game.locator('img.library-icon');
 await expect(icon).toBeVisible();
 await expect.poll(()=>icon.evaluate((el:HTMLImageElement)=>el.complete&&el.naturalWidth>0)).toBe(true);
 const broken=page.getByRole('region',{name:'Tekken',exact:true});
 await expect(broken.locator('img.library-icon')).toHaveCount(0);
 await expect(broken.locator('div.library-icon svg')).toBeVisible();
 await page.getByRole('button',{name:'Refresh list',exact:true}).click();
 await expect(icon).toBeVisible();
 await expect(broken.locator('img.library-icon')).toHaveCount(0);
 expect(await page.evaluate(()=>(window as any).libraryCalls)).toEqual([]);
});

test('game scope selection targets the game and locks until its cache is stopped',async({page})=>{
 await libraryFixture(page,false,false,true);
 const game=page.getByRole('region',{name:'Dragon Ball',exact:true});
 const mode=game.getByRole('combobox',{name:'Cache scope Dragon Ball',exact:true});
 await expect(mode).toBeDisabled();
 await game.getByRole('button',{name:'Stop & release space',exact:true}).click();
 await expect(mode).toBeEnabled();await mode.selectOption('dataOnly');
 await expect(mode).toHaveValue('dataOnly');await expect(game).toContainText('Game\\Content\\Paks');
 await expect(game).toContainText('Compatibility still depends on the game.');
 await game.getByRole('button',{name:'Enable cache',exact:true}).click();await expect(mode).toBeDisabled();
 await page.getByRole('button',{name:'Refresh list',exact:true}).click();await expect(mode).toHaveValue('dataOnly');
 const calls=await page.evaluate(()=>(window as any).libraryCalls);
 expect(calls.map((c:any)=>[c.operation,c.id])).toEqual([['stop','a'.repeat(32)],['modeData','a'.repeat(32)],['start','a'.repeat(32)]]);
 const other=page.getByRole('region',{name:'Tekken',exact:true});
 await expect(other.getByRole('combobox',{name:'Cache scope Tekken'}).locator('option[value="dataOnly"]')).toBeDisabled();
 await expect(other).toContainText('No suitable data folder detected.');
 await game.screenshot({path:'output/playwright/library-data-scope-en.png',animations:'disabled'});
});

test('Arabic data scope is readable at desktop widths',async({page})=>{
 await libraryFixture(page,true,false,true);
 const game=page.getByRole('region',{name:'Dragon Ball',exact:true});
 await game.getByRole('button',{name:'إيقاف وتحرير المساحة',exact:true}).click();
 const mode=game.getByRole('combobox',{name:'نطاق الكاش Dragon Ball',exact:true});
 await mode.selectOption('dataOnly');await expect(game).toContainText('التوافق يبقى حسب اللعبة.');
 for(const width of [1440,1050]){await page.setViewportSize({width,height:940});expect(await page.locator('main').evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true)}
 await game.screenshot({path:'output/playwright/library-data-scope-ar.png',animations:'disabled'});
});
test('Home restores the drive overview and live totals separately from Library',async({page})=>{
 await libraryFixture(page);
 await page.locator('nav').getByRole('button',{name:'Home',exact:true}).click();
 await expect(page.locator('.dashboard-hero')).toBeVisible();
 await expect(page.locator('.hero-device.source')).toContainText('Test D:');
 await expect(page.locator('.hero-device.destination')).toContainText('Test M:');
 await expect(page.locator('.status-pill')).toHaveText('Selected folder cache active');
 await expect(page.locator('.four-grid')).toContainText('6.00 GB');
 await expect(page.locator('.home-metrics-scope')).toHaveText('Live totals across all cache configurations');
 await expect(page.getByTestId('library-app')).toHaveCount(0);
 await expect(page.locator('.notice-bar')).not.toContainText('No application acceleration controls');
 await page.screenshot({path:'output/playwright/home-restored-en.png',animations:'disabled'});
 await page.getByRole('button',{name:'Open library',exact:true}).click();
 await expect(page.getByTestId('library-app')).toHaveCount(2);
 await expect(page.locator('.dashboard-hero')).toHaveCount(0);
 expect(await page.evaluate(()=>(window as any).libraryCalls)).toEqual([]);
});

test('Home uses saved drive configurations and stays readable in Arabic',async({page})=>{
 await libraryFixture(page);
 await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();
 await page.getByRole('button',{name:'Add configuration',exact:true}).click();
 const editor=page.getByRole('form',{name:'Cache configuration',exact:true});
 await editor.getByRole('textbox',{name:'Name (optional)'}).fill('Second cache');
 await editor.getByRole('combobox',{name:'Cache SSD'}).selectOption(JSON.stringify(['ssd-f','F:']));
 await editor.getByRole('button',{name:'Save configuration',exact:true}).click();
 await expect(page.getByRole('region',{name:'Second cache',exact:true})).toBeVisible();
 await page.locator('nav').getByRole('button',{name:'Home',exact:true}).click();
 await page.getByRole('combobox',{name:'Cache configuration',exact:true}).selectOption('y'.repeat(32));
 await expect(page.locator('.hero-device.source')).toContainText('Test E:');
 await expect(page.locator('.hero-device.destination')).toContainText('Test F:');
 await expect(page.locator('.four-grid')).toContainText('6.00 GB');
 await page.getByRole('button',{name:'العربية',exact:true}).click();
 await expect(page.getByRole('button',{name:'فتح المكتبة',exact:true})).toBeVisible();
 for(const width of [1440,1050]){
  await page.setViewportSize({width,height:940});
  expect(await page.locator('main').evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
 }
 await page.setViewportSize({width:1440,height:940});
 await page.screenshot({path:'output/playwright/home-restored-ar.png',animations:'disabled'});
});

test('library clears one game while preserving the other and targets its identity',async({page})=>{
 await libraryFixture(page);const first=page.getByRole('region',{name:'Dragon Ball'});const second=page.getByRole('region',{name:'Tekken'});
 await expect(first).toContainText('4.00 GB');await expect(second).toContainText('2.00 GB');
 await expect(first).toContainText('Uses the shared pool — no fixed quota');
 await expect(first.getByRole('progressbar')).toHaveCount(0);
 await expect(page.getByRole('region',{name:'Cache overview',exact:true})).toContainText('Automatic loading priority');
 await first.getByRole('button',{name:'Clear this cache',exact:true}).click();await expect(first).toContainText('0 B');await expect(second).toContainText('2.00 GB');
 const calls=await page.evaluate(()=>(window as any).libraryCalls);expect(calls).toEqual([{operation:'clear',id:'a'.repeat(32)}]);
});
test('library search, drive eligibility and independent pause controls',async({page})=>{
 await libraryFixture(page);await expect(page.getByTestId('library-app')).toHaveCount(2);
 await page.getByRole('button',{name:/^Installed games & apps/}).click();
 await page.getByRole('checkbox',{name:'Show unconfigured drives'}).check();const editor=page.getByRole('region',{name:'Editor'});await expect(editor.getByRole('button',{name:'Enable cache'})).toBeDisabled();
 await page.getByRole('textbox',{name:'Search games and apps'}).fill('Dragon');await expect(page.getByTestId('library-app')).toHaveCount(1);
 await page.getByRole('region',{name:'Dragon Ball'}).getByRole('button',{name:'Pause',exact:true}).click();await expect(page.getByRole('region',{name:'Dragon Ball'}).getByRole('status')).toHaveText('Paused');
 await page.getByRole('textbox',{name:'Search games and apps'}).fill('Tekken');await expect(page.getByRole('region',{name:'Tekken'}).getByRole('status')).toHaveText('Running');
});
test('library folder browser adds a named subtree without starting its cache',async({page})=>{
 await libraryFixture(page);await page.getByRole('button',{name:'Add a folder',exact:true}).click();await page.getByRole('textbox',{name:'Name (optional)'}).fill('My collection');await page.getByRole('button',{name:'Browse…'}).click();await page.getByRole('button',{name:'Add to library',exact:true}).click();
 const added=page.getByRole('region',{name:'My collection'});await expect(added).toContainText('D:\\Games\\Collection');await expect(added).toContainText('Includes all subfolders.');await expect(added.getByRole('status')).toHaveText('Stopped');
 expect(await page.evaluate(()=>(window as any).libraryCalls)).toEqual([{operation:'add',path:'D:\\Games\\Collection',name:'My collection'}]);
});
test('library uses one total budget and Arabic layout has no horizontal overflow',async({page})=>{
 await libraryFixture(page,true);await page.getByRole('button',{name:'إعدادات الكاش',exact:true}).last().click();await page.getByRole('button',{name:'تعديل التكوين',exact:true}).click();await page.getByRole('spinbutton',{name:'سعة هذا التكوين (GB)'}).fill('20');await page.getByRole('button',{name:'حفظ التكوين',exact:true}).click();
 const calls=await page.evaluate(()=>(window as any).libraryCalls);expect(calls[0].operation).toBe('save');expect(calls[0].configuration.budgetBytes).toBe(Math.floor(20e9/65536)*65536);
 await page.getByRole('button',{name:'إدارة الألعاب والكاش',exact:true}).click();
 await expect(page.getByRole('region',{name:'Dragon Ball'})).toContainText('دون حصة ثابتة');await expect(page.getByRole('region',{name:'Tekken'})).toContainText('دون حصة ثابتة');await expect(page.getByRole('region',{name:'ملخص الكاش',exact:true})).toContainText('20.00 GB');
 expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);await page.screenshot({path:'output/playwright/library-ar.png',fullPage:true,animations:'disabled'});
});
test('cached files stay grouped, use relative paths and can be searched and sorted',async({page})=>{
 await libraryFixture(page);
 const overview=page.getByRole('region',{name:'Cache overview',exact:true});
 await expect(overview).toContainText('25.00 GB');await expect(overview).toContainText('6.00 GB');await expect(overview).toContainText('19.00 GB');
 const game=page.getByRole('region',{name:'Dragon Ball',exact:true});const other=page.getByRole('region',{name:'Tekken',exact:true});
 await expect(game.getByRole('list',{name:'Cached files',exact:true})).not.toBeVisible();
 await game.locator('summary').click();const files=game.getByRole('list',{name:'Cached files',exact:true});
 await expect(files.getByRole('listitem')).toHaveCount(3);await expect(files.getByRole('listitem').first()).toContainText('assets.pak');
 await expect(files).toContainText('4.00 KB');await expect(files).toContainText('256.0 KB');await expect(files).not.toContainText('D:\\Games');await expect(files).not.toContainText('0 MB');
 await game.getByRole('combobox',{name:'Sort cached files'}).selectOption('name');await expect(files.getByRole('listitem').nth(1)).toContainText('Textures.ucas');
 await game.getByRole('textbox',{name:'Search cached files'}).fill('textures');await expect(files.getByRole('listitem')).toHaveCount(1);
 await game.getByRole('textbox',{name:'Search cached files'}).fill('missing');await expect(game).toContainText('No cached files match your search.');
 await expect(other.getByRole('list',{name:'Cached files',exact:true})).not.toBeVisible();
 await game.getByRole('textbox',{name:'Search cached files'}).fill('');
 await page.screenshot({path:'output/playwright/cache-library-en.png',fullPage:true,animations:'disabled'});
});
test('cache settings lead back to the library without duplicated game controls',async({page})=>{
 await libraryFixture(page);
 await page.getByRole('region',{name:'Cache overview',exact:true}).getByRole('button',{name:'Cache settings',exact:true}).click();
 await expect(page.getByRole('heading',{name:'Cache settings',exact:true})).toBeVisible();
 await page.screenshot({path:'output/playwright/cache-settings-redesign-en.png',fullPage:true,animations:'disabled'});
 await expect(page.getByRole('button',{name:'Clear this cache',exact:true})).toHaveCount(0);
 await expect(page.getByTestId('cache-contents')).toHaveCount(0);
 await page.getByRole('button',{name:'Manage games & cache',exact:true}).click();
 await expect(page.getByRole('region',{name:'Dragon Ball',exact:true})).toBeVisible();
});
test('Arabic expanded cache details remain readable at both desktop widths',async({page})=>{
 await libraryFixture(page,true);const game=page.getByRole('region',{name:'Dragon Ball',exact:true});await game.locator('summary').click();
 for(const width of [1440,1050]){
  await page.setViewportSize({width,height:940});
  expect(await page.locator('main').evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
 }
 await page.setViewportSize({width:1440,height:940});await page.screenshot({path:'output/playwright/cache-library-ar.png',fullPage:true,animations:'disabled'});
 await game.locator('.library-file-list').scrollIntoViewIfNeeded();await page.screenshot({path:'output/playwright/cache-library-ar-files.png',animations:'disabled'});
});
test('diagnostics replaces removed tools and describes the current folder cache',async({page})=>{
 await libraryFixture(page);
 await expect(page.locator('nav > button')).toHaveCount(6);
 await expect(page.locator('nav').getByRole('button',{name:/^(Downloads|Gaming|Advanced)$/})).toHaveCount(0);
 await expect(page.locator('.nav-tools')).toHaveCount(0);
 await page.locator('nav').getByRole('button',{name:'Performance & diagnostics',exact:true}).click();await page.locator('.performance-diagnostics>summary').click();
 await expect(page.getByRole('heading',{name:'Performance & diagnostics',exact:true})).toBeVisible();
 await expect(page.locator('main')).toContainText('Selected-folder cache using the installed Dokan runtime.');
 await expect(page.locator('main')).toContainText('Cache host');
 await expect(page.locator('main')).not.toContainText('Windows service');
 await expect(page.locator('main')).not.toContainText('NotInstalled');
 await expect(page.locator('main')).not.toContainText('Driver qualification gate');
 await page.screenshot({path:'output/playwright/diagnostics-en.png',animations:'disabled'});
 await page.getByRole('button',{name:'العربية',exact:true}).click();
 await expect(page.locator('.performance-diagnostics>summary')).toHaveText('التشخيص');
 await expect(page.locator('main')).toContainText('مشغّل الكاش');
 await page.screenshot({path:'output/playwright/diagnostics-ar.png',animations:'disabled'});
 await page.locator('main').getByRole('button',{name:'المكتبة',exact:true}).click();
 await expect(page.getByRole('region',{name:'Dragon Ball',exact:true})).toBeVisible();
});
test('configurations select another HDD and SSD and stop or delete independently',async({page})=>{
 await libraryFixture(page);await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();
 await page.getByRole('button',{name:'Add configuration',exact:true}).click();const editor=page.getByRole('form',{name:'Cache configuration',exact:true});
 await editor.getByRole('textbox',{name:'Name (optional)'}).fill('Second cache');
 await expect(editor.getByRole('combobox',{name:'Source HDD'})).toHaveValue(JSON.stringify(['source-e','E:']));
 await editor.getByRole('combobox',{name:'Cache SSD'}).selectOption(JSON.stringify(['ssd-f','F:']));
 await editor.getByRole('spinbutton',{name:'Capacity for this configuration (GB)'}).fill('17.5');await editor.getByRole('button',{name:'Save configuration',exact:true}).click();
 const other=page.getByRole('region',{name:'Second cache',exact:true});await expect(other).toContainText('17.50 GB');await expect(page.getByRole('region',{name:'D to M',exact:true})).toContainText('25.00 GB');
 await page.getByRole('button',{name:'Manage games & cache',exact:true}).click();await page.getByRole('button',{name:/^Installed games & apps/}).click();
 await page.getByRole('region',{name:'Racer',exact:true}).getByRole('button',{name:'Enable cache',exact:true}).click();
 await expect(page.getByRole('region',{name:'Racer',exact:true})).toContainText('E: → F:');
 await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();await other.getByRole('button',{name:'Edit configuration',exact:true}).click();
 await expect(editor.getByRole('combobox',{name:'Cache SSD'})).toBeDisabled();await editor.getByRole('button',{name:'Close',exact:true}).click();
 await other.getByRole('button',{name:'Stop configuration',exact:true}).click();await other.getByRole('button',{name:'Edit configuration',exact:true}).click();
 await expect(editor.getByRole('combobox',{name:'Cache SSD'})).toBeEnabled();await editor.getByRole('combobox',{name:'Cache SSD'}).selectOption(JSON.stringify(['ssd-m','M:']));await editor.getByRole('button',{name:'Save configuration',exact:true}).click();
 await expect(other.getByRole('group',{name:'E: → M:',exact:true})).toBeVisible();await other.getByRole('button',{name:'Delete configuration',exact:true}).click();await other.getByRole('button',{name:'Stop and delete configuration',exact:true}).click();await expect(other).toHaveCount(0);
 await expect(page.getByRole('region',{name:'D to M',exact:true})).toContainText('2');
 const calls=await page.evaluate(()=>(window as any).libraryCalls);expect(calls[0].configuration).toMatchObject({sourceDiskId:'source-e',sourceLetter:'E:',ssdDiskId:'ssd-f',ssdLetter:'F:'});
});
test('configuration capacity typing and Arabic device layout stay usable',async({page})=>{
 await libraryFixture(page);await page.locator('nav').getByRole('button',{name:'Cache settings',exact:true}).click();await page.getByRole('button',{name:'Edit configuration',exact:true}).click();
 const capacity=page.getByRole('spinbutton',{name:'Capacity for this configuration (GB)'});await capacity.fill('');await capacity.pressSequentially('12.5');await expect(capacity).toHaveValue('12.5');await expect(capacity).toBeFocused();
 await page.screenshot({path:'output/playwright/cache-settings-editor-en.png',fullPage:true,animations:'disabled'});
 await page.getByRole('form',{name:'Cache configuration',exact:true}).getByRole('button',{name:'Save configuration',exact:true}).click();
 await page.getByRole('button',{name:'العربية',exact:true}).click();
 for(const width of [1440,1050]){await page.setViewportSize({width,height:940});expect(await page.locator('main').evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true);await page.screenshot({path:`output/playwright/cache-settings-ar-${width}.png`,fullPage:true,animations:'disabled'})}
 await page.setViewportSize({width:1440,height:940});await page.screenshot({path:'output/playwright/cache-configurations-ar.png',animations:'disabled'});
});
