// Consolidate the v5 candidate; retain earlier snapshots without requalifying them.
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..');
const read=file=>JSON.parse(fs.readFileSync(path.join(root,file),'utf8').replace(/^\uFEFF/,''));
const text=file=>fs.readFileSync(path.join(root,file),'utf8');
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex').toUpperCase();
const rust=text('artifacts/direct-v5-tests.log');
const clippy=text('artifacts/direct-v5-clippy.log');
assert(!/test result: FAILED|error(?:\[|:)/.test(rust));
const rustTests=[...rust.matchAll(/test result: ok\. (\d+) passed/g)].reduce((sum,m)=>sum+Number(m[1]),0);
assert.equal(rustTests,103);
assert(/Finished `dev` profile/.test(clippy)&&!/error(?:\[|:)/.test(clippy));
const ui=read('output/playwright/results.json');
assert.equal(ui.stats.expected,15);assert.equal(ui.stats.unexpected,0);
assert.equal(ui.stats.flaky,0);assert.equal(ui.stats.skipped,0);
const policy=read('artifacts/direct-policy-test/result.json');
assert(policy.passed&&policy.checks===44);
for(const name of ['driver-direct-analysis','driver-direct-vm-analysis']){
 assert(/<DEFECTS\s*><\/DEFECTS>|<DEFECTS\s*\/>/.test(text(`artifacts/${name}/driver.nativecodeanalysis.xml`)));
}
const installer=read('artifacts/direct-v5-installer-checks.json');
const guard=read('artifacts/direct-v5-release-guard.json');
assert(installer.passed&&installer.nativeHelperCompiled&&!installer.hardwareMutated);
assert(guard.passed&&guard.unqualifiedHelperRefused&&guard.protectedSetupUnchanged&&guard.unsignedPackageRefused&&guard.packageNotCreated);
assert.equal(guard.serviceSha256,hash('target/release/SuperHDDService.exe'));
const stamp=new Date().toISOString().slice(0,10).replaceAll('-','');
const native=read(`artifacts/runtime/native-audit-final-${stamp}.json`);
const startup=read(`artifacts/runtime/native-final-startup-${stamp}.json`);
assert(Object.values(native.checks).every(v=>v===true));
assert(native.checks.nativeV5ReleaseGate&&native.checks.nativeV5PrivilegedActionRefused);
assert(startup.running&&startup.debugPortClosed&&startup.debugEnvironmentAbsent);
assert.equal(startup.sha256,hash('target/release/SuperHDD.exe'));
assert(Date.parse(native.at)>=fs.statSync(path.join(root,'target/release/SuperHDD.exe')).mtimeMs);
const desktopInstaller=read(`artifacts/runtime/installer-test-${stamp}.json`);
for(const [key,value]of Object.entries(desktopInstaller))if(typeof value==='boolean')assert(value,key);
assert(Date.parse(desktopInstaller.at)>=fs.statSync(path.join(root,'target/release/SuperHDD.exe')).mtimeMs);
const vm=read('artifacts/runtime/kernel-lab/direct-session.json');
const vmSourcesCurrent=vm.sourceHashes?.length>=40&&vm.sourceHashes.every(s=>fs.existsSync(s.Path)&&hash(path.relative(root,s.Path))===s.Hash);
const vmPayloadsCurrent=vm.binaryHashes?.length===2&&vm.binaryHashes.every(b=>hash(b.name==='SuperHDDService.exe'?'target/release/SuperHDDService.exe':`target/release/examples/${b.name}`)===b.sha256)&&vm.directDriver?.unsignedSha256===hash('artifacts/driver-direct-vm/SuperHDDCacheDriver.sys');
const currentVm=!!(vm.phase==='completed'&&vm.passed&&vm.probe?.passed&&Object.values(vm.probe.checks).every(v=>v===true)&&vm.probe?.ownedFixturesRemoved&&vm.hostParentsRemoved&&vm.coldBootCleanup?.passed&&vm.finalHost?.passed&&vm.finalHost?.vmState==='Off'&&vm.finalHost?.assignedVmMemoryBytes===0&&vm.finalHost.activeHostCacheDrivers?.length===0&&Date.parse(vm.finalHost.at)>=Date.parse(vm.at)&&vmSourcesCurrent&&vmPayloadsCurrent);
const sources=[];
function walk(directory){for(const entry of fs.readdirSync(path.join(root,directory),{withFileTypes:true})){
 const file=path.join(directory,entry.name);
 if(entry.isDirectory()){if(!['target','node_modules'].includes(entry.name))walk(file);}
 else if(/\.(?:rs|c|h|inc|toml|ps1|tsx?|css|json|cjs)$/.test(entry.name))sources.push(file);
}}
for(const directory of ['crates','services','apps/desktop/src','apps/desktop/src-tauri/src','drivers/superhdd-cache-driver','installer','tests'])walk(directory);
sources.push('Cargo.toml','Cargo.lock','package-lock.json','scripts/build-direct-driver.ps1','scripts/test-direct-session.ps1','scripts/package-qualified-runtime.ps1','scripts/record-direct-v5-audit.cjs','scripts/test-native-ui.cjs');
const payloads=['target/release/SuperHDD.exe','target/release/SuperHDDService.exe','target/release/examples/direct_session_probe.exe','artifacts/driver-direct-analysis/SuperHDDCacheDriver.sys','artifacts/driver-direct-vm/SuperHDDCacheDriver.sys'];
const report={schema:1,at:new Date().toISOString(),scope:'Direct-v5 development candidate; production branch compiled/static-tested only',localAuditPassed:true,rustTests,browserTests:ui.stats.expected,driverPolicyChecks:policy.checks,driverStaticAnalysisPassed:true,installerSourceChecksPassed:true,unsignedReleaseRefusalPassed:true,nativeDesktopChecks:Object.keys(native.checks).length,desktopInstallerPassed:true,vmRuntimeProbePassed:!!vm.probe?.passed,vmFullCurrentRunPassed:currentVm,vmRunId:vm.runId,vmError:vm.error||vm.cleanupError||vm.shutdownError||null,vmShutdownCurrentlyProven:currentVm,physicalSourceQualified:false,productionKernelSignatureAvailable:false,productionRuntimeInstallationQualified:false,productionReady:false,sourceHashes:sources.map(file=>({file,sha256:hash(file)})),payloadHashes:payloads.map(file=>({file,sha256:hash(file)}))};
fs.writeFileSync(path.join(root,'artifacts/direct-v5-evidence.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({...report,sourceHashes:`${report.sourceHashes.length} inputs`,payloadHashes:`${report.payloadHashes.length} payloads`},null,2));
