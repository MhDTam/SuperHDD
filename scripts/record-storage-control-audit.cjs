// Independently bind the native container proof to its binary and source inputs.
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..');
const read=file=>JSON.parse(fs.readFileSync(path.join(root,file),'utf8').replace(/^\uFEFF/,''));
const hash=file=>crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex').toUpperCase();
const result=read('artifacts/runtime/cache-container-result.json');
const lifecycle=read('artifacts/runtime/cache-lifecycle-result.json');
const guard=read('artifacts/runtime/cache-session-host-refusal.json');
const managed=read('artifacts/runtime/kernel-lab/managed-session.json');
const scm=read('artifacts/runtime/kernel-lab/scm-recovery.json');
const verification=read('artifacts/runtime/cache-control-verification.json');
assert(verification.passed && verification.phase==='completed' && verification.error===null);
assert(result.passed && result.phase==='completed' && result.error===null);
assert(result.hostDriverInstalled===false && result.hostSecurityChanged===false && result.sourceDataWritten===false && result.globalAccelerationActive===false);
assert.equal(result.toolSha256,hash('target/release/examples/container_probe.exe'));
for(const file of ['crates/storage-control/src/lib.rs','crates/storage-control/src/windows.rs','crates/storage-control/src/journal.rs','crates/storage-control/src/format.rs','crates/storage-control/src/raw_volume.rs','crates/storage-control/src/session.rs','crates/storage-control/examples/container_probe.rs','scripts/test-cache-container.ps1']){
  assert(result.sourceHashes.some(item=>item.Path===path.join(root,file) && item.Hash===hash(file)),'Container proof source changed: '+file);
}
const probe=result.probe;
assert(probe.passed && probe.rootRemoved && Object.values(probe.checks).every(v=>v===true));
assert(Object.values(probe.cleanup).every(v=>v===true));
for(const file of [probe.ownedPath,result.root]) assert(!fs.existsSync(file),'Owned container fixture remained');
assert(probe.budget.totalBytes<=25_000_000_000 && probe.containerFileBytes+8192<=probe.budget.totalBytes);
assert(probe.sourceD.diskNumber!==probe.actualParent.diskNumber && probe.sourceD.incursSeekPenalty===true && probe.actualParent.incursSeekPenalty===false);
assert(probe.dependencies.length===1 && probe.dependencies[0].ancestorLevel===0 && probe.dependencies[0].flags===194);
assert.equal(probe.logicalDriveMaskBefore,probe.logicalDriveMaskAfter);
assert(lifecycle.passed && lifecycle.phase==='completed' && lifecycle.error===null && lifecycle.cycles.length===2);
assert.equal(lifecycle.toolSha256,hash('target/release/examples/container_lifecycle_probe.exe'));
for(const item of lifecycle.sourceHashes) assert.equal(item.Hash,hash(path.relative(root,item.Path)));
for(const cycle of lifecycle.cycles){
  assert(cycle.prepared.passed && cycle.recovered.passed && cycle.recovered.removed && cycle.rootRemoved);
  assert.deepEqual(cycle.prepared.instance,cycle.recovered.instance);
  assert.equal(cycle.prepared.ownedPath,cycle.recovered.ownedPath);
  assert(!fs.existsSync(cycle.prepared.ownedPath) && !fs.existsSync(path.dirname(cycle.prepared.ownedPath)));
  for(const stage of [cycle.prepared,cycle.recovered]){
    assert(Object.values(stage.checks).every(v=>v===true));
    assert(stage.budget.totalBytes<=25_000_000_000 && stage.fileBytes+8192<=stage.budget.totalBytes);
    assert(stage.rawVolume.capacityBytes<stage.budget.virtualDiskBytes);
  }
}
assert(lifecycle.cycles.some(c=>c.mode==='hold' && c.processKilled));
assert(guard.passed && guard.driverMissingSurfaced && guard.rootNotCreated && guard.isolatedLabHostRefused && guard.scmPrepareHostRefused);
assert.equal(guard.serviceSha256,hash('target/release/SuperHDDService.exe'));
for(const item of guard.sourceHashes) assert.equal(item.Hash,hash(path.relative(root,item.Path)));
const log=fs.readFileSync(path.join(root,'artifacts/cache-control-workspace-tests.log'),'utf8');
assert(!/test result: FAILED/.test(log));
const tests=[...log.matchAll(/test result: ok\. (\d+) passed/g)].reduce((n,m)=>n+Number(m[1]),0);
assert.equal(tests,96);
const clippy=fs.readFileSync(path.join(root,'artifacts/cache-control-clippy.log'),'utf8');
assert(/Finished `dev` profile/.test(clippy) && !/error(?:\[|:)/.test(clippy));
assert(managed.passed && managed.phase==='completed' && managed.error===null && !managed.cleanupError && !managed.shutdownError);
assert(managed.probe.passed && managed.probe.ownedFixturesRemoved && Object.values(managed.probe.checks).every(v=>v===true));
assert(managed.probe.automaticCrashRecoveryProven===false && managed.physicalMediaProof===false && managed.hostDriverInstalled===false);
const guestCleanup=Array.isArray(managed.guestCleanup)?managed.guestCleanup:[managed.guestCleanup];
const completedCleanup=guestCleanup.filter(item=>item?.cacheDetached===true && item.childRemoved===true && item.ownedProcessesAbsent===true);
assert(managed.hostParentsRemoved && completedCleanup.length===1);
for(const item of guestCleanup.filter(item=>Object.hasOwn(item,'Attached'))){
  assert(item.Attached===false && item.ImagePath===managed.source.path,'Wrong or still attached source image during cleanup');
}
assert(managed.finalHost.passed && managed.finalHost.vmState==='Off' && managed.finalHost.assignedVmMemoryBytes===0);
assert.equal(managed.preparation.driverHash,hash('artifacts/runtime/kernel-lab/signed-driver.sys'));
assert.equal(managed.preparation.signature,'Valid');
for(const [name,file] of Object.entries({'SuperHDDService.exe':'target/release/SuperHDDService.exe','managed_session_probe.exe':'target/release/examples/managed_session_probe.exe'})){
  assert(managed.binaryHashes.some(item=>item.name===name && item.sha256===hash(file) && item.guestHashVerified===true),'Managed probe binary changed after proof: '+name);
}
for(const item of managed.sourceHashes) assert.equal(item.Hash,hash(path.relative(root,item.Path)));
for(const parent of managed.parents) assert(!fs.existsSync(parent.path),'Run-owned parent file remains');
assert(managed.probe.warm.cache.hits>managed.probe.before.cache.hits && managed.probe.warm.cache.ssdReadBytes>managed.probe.before.cache.ssdReadBytes);
assert(managed.probe.imported.importedRecords>0 && managed.probe.revalidated.revalidatedRecords>0);
assert(scm.passed && scm.phase==='completed' && scm.error===null && !scm.cleanupError && !scm.shutdownError);
assert(scm.probe.passed && scm.probe.ownedFixturesRemoved && Object.values(scm.probe.checks).every(v=>v===true));
assert(scm.probe.killedPid>0 && scm.probe.restartedPid>0 && scm.probe.killedPid!==scm.probe.restartedPid);
assert(scm.probe.initial.localSystem && scm.probe.recovered.localSystem && scm.probe.recovered.recoveries===1);
assert(scm.probe.recovered.recovery.kernelDisabled && scm.probe.recovered.recovery.staleBackingDetached);
assert.deepEqual(scm.probe.initial.target.containerInstance,scm.probe.recovered.target.containerInstance);
assert(Object.values(scm.probe.acl).every(v=>v===true));
assert(scm.probe.foreignBefore.sourceDiskNumber===scm.probe.foreignAfter.sourceDiskNumber && scm.probe.foreignBefore.backingDiskNumber===scm.probe.foreignAfter.backingDiskNumber);
for(const key of ['attached','paused','generation','capacityBytes','sourceLength']) assert.equal(scm.probe.foreignBefore.cache[key],scm.probe.foreignAfter.cache[key],'Foreign kernel session changed: '+key);
assert(scm.probe.foreignAfter.cache.attached && scm.probe.foreignRefusal.phase==='faulted');
assert(scm.probe.imported.importedRecords>0 && scm.probe.afterRecoveryReads.revalidatedRecords>0 && scm.probe.afterRecoveryReads.cache.errors===0);
assert(scm.guestCleanup.cacheDetached && scm.guestCleanup.childRemoved && scm.guestCleanup.ownedProcessesAbsent && scm.guestCleanup.ownedScmServiceAbsent && scm.guestCleanup.protectedStateRemoved && scm.hostParentsRemoved);
assert(scm.finalHost.passed && scm.finalHost.vmState==='Off' && scm.finalHost.assignedVmMemoryBytes===0);
assert(scm.hostDriverInstalled===false && scm.physicalMediaProof===false);
assert(scm.preparation.signature==='Valid' && scm.preparation.driverHash===hash('artifacts/runtime/kernel-lab/signed-driver.sys'));
for(const [name,file] of Object.entries({'SuperHDDService.exe':'target/release/SuperHDDService.exe','scm_recovery_probe.exe':'target/release/examples/scm_recovery_probe.exe'})){
  assert(scm.binaryHashes.some(item=>item.name===name && item.sha256===hash(file) && item.guestHashVerified===true),'SCM proof binary changed: '+name);
}
for(const item of scm.sourceHashes) assert.equal(item.Hash,hash(path.relative(root,item.Path)));
for(const parent of scm.parents) assert(!fs.existsSync(parent.path),'SCM parent file remains');
const evidence={recordedAt:new Date().toISOString(),evidenceAuditPassed:true,workspaceRustTests:tests,storageControlTests:35,serviceRequestTests:1,clippyPassed:true,
  nativeContainerProofPassed:true,containerBudgetBytes:probe.budget.totalBytes,actualContainerFileBytes:probe.containerFileBytes,
  sourceDisk:probe.sourceD.diskNumber,ssdDisk:probe.actualParent.diskNumber,sourceETopologyPassed:probe.sourceE.topologyPassed===true,
  sourceELimitation:'The E: device did not provide a seek-penalty result to this new native controller. It is not accepted by this controller; the existing UI has a separately documented narrow model fallback.',
  connectedToDesktopOrService:true,developerServiceOnly:true,physicalFilterEnrollmentPerformed:false,productionReady:false,globalAccelerationActive:false,
  nativeRawInitializationProofPassed:true,nativeNormalReopenProofPassed:true,nativeKilledProcessReopenProofPassed:true,
  twoSlotJournalImplemented:true,nativeSessionBackendImplemented:true,developerServiceSessionCommandImplemented:true,
  nativeManagedSessionPositiveRuntimeProofPassed:true,productionDesktopActivationConnected:false,
  managedSessionVmProof:managed,cacheControlVerification:verification,protectedScmOwnershipProven:true,scmRecoveryVmProof:scm,automaticCrashRecoveryProven:true,automaticCrashRecoveryScope:'LocalSystem SCM process-crash restart and exact-session reconciliation in the isolated VM',physicalMediaProof:false,
  lifecycleProof:{at:lifecycle.at,cycles:lifecycle.cycles,toolSha256:lifecycle.toolSha256},hostSessionRefusal:guard,
  hostDriverInstalled:false,hostSecurityChanged:false,hostSourceDataWritten:false,disposableGuestSourceWritten:true,testPayloadsRemoved:true,
  kernelSha256:hash('artifacts/driver-raw-lab/SuperHDDCacheDriver.sys'),probeBinarySha256:result.toolSha256,
  sourceHashes:[...new Map([...result.sourceHashes,...lifecycle.sourceHashes,...guard.sourceHashes,...managed.sourceHashes,...scm.sourceHashes].map(item=>[item.Path,item])).values()],nativeResultFile:result.report,
  scope:'Native VHDX/RAW ownership and actual service lifecycle against the signed isolated-VM driver, plus protected LocalSystem SCM ownership, automatic process-crash restart/reconciliation, restricted-user denial and foreign-session preservation. Recovered records were revalidated against source bytes. Power-loss/cold-boot and physical deployment are not qualified.',
  remaining:['Physical-source filter support and qualification','Power-loss/cold-boot and partial-initialization fault qualification','Production installer/rollback and desktop activation integration','Free-only signing budget: no eligible Microsoft kernel-driver signing sponsor or submission route has been confirmed','Microsoft signing and exact-image deployment qualification'],
  signingBudget:'free-only',freeKernelSigningRouteConfirmed:false,
  signingBrief:'docs/release/free-signing-research.md'};
fs.writeFileSync(path.join(root,'artifacts/storage-control-evidence.json'),JSON.stringify(evidence,null,2)+'\n');
console.log(JSON.stringify(evidence,null,2));
