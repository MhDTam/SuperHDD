import {test,expect,type Page} from '@playwright/test';
async function fixture(page:Page,variant='idle',arabic=false){
 await page.addInitScript(({variant,arabic})=>{
  localStorage.setItem('superhdd-welcome','1');localStorage.setItem('superhdd-language',arabic?'ar':'en');
  const m=(seconds:number)=>({seconds,p95Ms:2,MiBPerSecond:32});
  const valid=variant!=='invalid';const report={valid,publicationReady:true,statistics:{pairedGeometricMeanSpeedup:variant==='invalid'?99:variant==='inconclusive'?1:0.75,confidence95Lower:variant==='inconclusive'?.8:.6,confidence95Upper:variant==='inconclusive'?1.2:.9,verdict:variant==='inconclusive'?'inconclusive':'slower'},firstPass:{measurement:m(.5)},training:[{measurement:m(.8)}],references:{ssdDirect:{measurement:m(.02)},windowsWarm:{measurement:m(.01)}},pairs:Array.from({length:6},(_,i)=>({index:i+1,direct:{measurement:m(.3)},cached:{measurement:m(.4)}}))};
  let state:any={phase:variant==='idle'?'idle':'complete',running:false,canRecover:false,exportReady:variant!=='idle',report:variant==='idle'?null:report};const calls:any[]=[];
  Object.assign(window,{isTauri:true,benchmarkCalls:calls,__TAURI_INTERNALS__:{invoke:async(command:string,args?:any)=>{
   if(command==='benchmark_status')return structuredClone(state);
   if(command==='benchmark_action'){calls.push(args.command);if(args.command.action==='start')state={phase:'measuring',running:true,percent:50,canRecover:false,report:null};if(args.command.action==='cancel')state={phase:'cancelled',running:false,canRecover:false,report:{valid:false,publicationReady:false,pairs:[]}};return structuredClone(state)}
   if(command==='get_app_library')return {apps:[],configurations:[{id:'x'.repeat(32),name:'D to M',sourceLetter:'D:',ssdLetter:'M:',budgetBytes:25e9}],budgetBytes:25e9,available:true,legacyActive:false};
   if(command==='get_snapshot')return {disks:[],drivers:[],serviceState:'Running',collectedAt:'fixture'};
   if(command==='get_cache_telemetry')return null;
   if(command==='get_cache_runtime'||command==='get_folder_runtime')return {phase:'stopped',canStart:false,canControl:false,canRemove:false};
   throw Error(command);
  }}});
 },{variant,arabic});
 await page.goto('/');await page.locator('nav').getByRole('button',{name:arabic?'الأداء والتشخيص':'Performance & diagnostics',exact:true}).click();
}
test('benchmark start carries only a configuration and profile and supports cancellation',async({page})=>{
 await fixture(page);const tool=page.getByRole('region',{name:'Storage benchmark',exact:true});
 await tool.getByRole('combobox',{name:'Test profile'}).selectOption('quick');await tool.getByRole('button',{name:'Run benchmark',exact:true}).click();
 await expect(tool.getByRole('progressbar')).toHaveAttribute('value','50');await expect(tool.getByRole('combobox',{name:'Drive configuration'})).toBeDisabled();
 await tool.getByRole('button',{name:'Cancel test',exact:true}).click();await expect(tool).toContainText('Incomplete or invalid — no speed claim');await expect(tool.locator('.benchmark-score')).toHaveCount(0);
 expect(await page.evaluate(()=>(window as any).benchmarkCalls)).toEqual([{action:'start',configurationId:'x'.repeat(32),profile:'quick',workload:'mixed'},{action:'cancel'}]);
});
test('slower and inconclusive measurements are displayed without claiming success',async({page})=>{
 await fixture(page,'slower');const tool=page.getByRole('region',{name:'Storage benchmark',exact:true});
 await expect(tool).toContainText('0.75×');await expect(tool).toContainText('Slower in this storage test');await expect(tool.locator('tbody tr')).toHaveCount(6);await expect(tool).toContainText('Windows-buffered pre-read source');
 await tool.getByRole('button',{name:'Open shareable ZIP'}).click();expect(await page.evaluate(()=>(window as any).benchmarkCalls)).toEqual([{action:'openFolder'}]);
 await tool.screenshot({path:'output/playwright/benchmark-tool-slower-en.png',animations:'disabled'});
});
test('invalid data hides a fabricated speed ratio',async({page})=>{
 await fixture(page,'invalid');const tool=page.getByRole('region',{name:'Storage benchmark',exact:true});await expect(tool).toContainText('Incomplete or invalid');await expect(tool.locator('.benchmark-score')).toHaveCount(0);await expect(tool).not.toContainText('99.00×');
});
test('Arabic benchmark and confidence interval remain readable',async({page})=>{
 await fixture(page,'inconclusive',true);const tool=page.getByRole('region',{name:'بنشمارك التخزين',exact:true});await expect(tool).toContainText('فرق السرعة غير حاسم');
 for(const width of [1440,1050]){await page.setViewportSize({width,height:940});expect(await page.locator('main').evaluate(el=>el.scrollWidth<=el.clientWidth+1)).toBe(true)}
 await tool.screenshot({path:'output/playwright/benchmark-tool-ar.png',animations:'disabled'});
});
