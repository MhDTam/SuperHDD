// Consolidate recorded evidence; this never enables an engine or installs anything.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const root = path.resolve(__dirname, '..');
const read = file => JSON.parse(fs.readFileSync(path.join(root, file), 'utf8').replace(/^\uFEFF/, ''));
const native = read('artifacts/runtime/native-audit-final-20261019.json');
const ui = read('output/playwright/results.json');
const crash = read('artifacts/runtime/download-crash-result.json');
const installer = read('artifacts/runtime/installer-test-20261019.json');
const instance = read('artifacts/runtime/single-instance-20261019.json');
const startup = read('artifacts/runtime/native-final-startup-20261019.json');
const bridgeGuard = read('artifacts/runtime/native-bridge-host-guard.json');
const priorBridgeAttempt = read('artifacts/runtime/native-bridge-attempt.json');
const bridge = read('artifacts/runtime/kernel-lab/native-bridge.json');
const productionSignature = read('artifacts/runtime/kernel-lab/host-production-signature-check.json');
const bridgeHost = read('artifacts/runtime/native-bridge-host-observation.json');
const storageControl = read('artifacts/storage-control-evidence.json');
const environment = read('artifacts/runtime/driver-environment-20261018.json');
const hyperv = read('artifacts/runtime/hyperv-query-20261018.json');
const lab = read('artifacts/runtime/lab-preflight-v2.json');
const pnpLab = read('artifacts/runtime/lab-preflight-v3.json');
const pnpGuard = read('artifacts/runtime/pnp-host-guard-result.json');
const driverState = read('artifacts/driver-state-test/result.json');
const rawStore = read('artifacts/raw-store-test/result.json');
const guest = read('artifacts/runtime/kernel-lab/action-status.json');
const provision = read('artifacts/runtime/kernel-lab/provisioning.json');
const signing = read('artifacts/runtime/kernel-lab/action-sign.json');
const resumedProbe = fs.existsSync(path.join(root,'artifacts/runtime/kernel-lab/action-resumeprobe.json')) ? read('artifacts/runtime/kernel-lab/action-resumeprobe.json') : null;
const uac = read('artifacts/runtime/kernel-lab/uac-status.json');
const kernelProbe = fs.existsSync(path.join(root,'artifacts/runtime/kernel-lab/probe.json')) ? read('artifacts/runtime/kernel-lab/probe.json') : null;
const hardwareBenchmark = fs.existsSync(path.join(root,'artifacts/runtime/kernel-lab/hardware-benchmark.json')) ? read('artifacts/runtime/kernel-lab/hardware-benchmark.json') : null;
const cancellationStress = read('artifacts/runtime/kernel-lab/cancellation-stress.json');
const verifierStress = read('artifacts/runtime/kernel-lab/verifier-stress.json');
const pendingStress = read('artifacts/runtime/kernel-lab/verifier-pending-stress.json');
const finalHost = read('artifacts/runtime/kernel-lab/final-host-state.json');
const driverBuild = read('artifacts/driver-build-evidence.json');
const cleanup = read('artifacts/runtime/benchmark-20261018/cleanup.json');
assert(Object.values(native.checks).every(value => value === true));
assert.equal(ui.stats.unexpected, 0); assert.equal(ui.stats.skipped, 0);
assert.equal(ui.stats.flaky, 0); assert.equal(ui.stats.expected, 12);
assert(bridgeGuard.passed && bridgeGuard.hostRefused && bridgeGuard.reportNotCreated);
assert(bridge.passed && bridge.phase==='completed' && bridge.error===null && !bridge.cleanupError && !bridge.shutdownError);
assert(bridge.probe.passed && bridge.probe.fixtureRemoved && bridge.probe.readOperations===12 && bridge.probe.readBytes===786432);
assert(Object.values(bridge.probe.checks).every(value=>value===true));
assert(bridge.cleanup.serviceAbsent && bridge.cleanup.cacheDetached && bridge.finalHost.passed && bridge.finalHost.vmState==='Off');
assert(bridge.hostServiceInstalled===false && bridge.vendorUsed===false);
assert(bridgeHost.activeHostCacheDrivers.length===0 && bridgeHost.applicationRunning===false && bridgeHost.debugPortClosed);
assert(storageControl.evidenceAuditPassed && storageControl.nativeContainerProofPassed && storageControl.nativeKilledProcessReopenProofPassed && storageControl.nativeManagedSessionPositiveRuntimeProofPassed && storageControl.workspaceRustTests===96);
assert(storageControl.protectedScmOwnershipProven && storageControl.automaticCrashRecoveryProven && storageControl.scmRecoveryVmProof.passed);
assert(storageControl.connectedToDesktopOrService===true && storageControl.developerServiceOnly===true && storageControl.productionDesktopActivationConnected===false && storageControl.globalAccelerationActive===false && storageControl.testPayloadsRemoved);
assert(driverState.passed && driverState.checks === 188);
assert(rawStore.passed && rawStore.checks >= 120834);
assert(!guest.error && guest.guest.build === '20348');
assert(provision.generation === 1 && provision.networkAdapters === 0 && !provision.hostSecurityChanged);
assert(finalHost.passed && finalHost.vmState==='Off' && finalHost.assignedVmMemoryBytes===0 && finalHost.activeHostCacheDrivers.length===0);
assert(Date.parse(finalHost.at)>=Math.max(...[cancellationStress,verifierStress,pendingStress,resumedProbe,bridge,storageControl.managedSessionVmProof,storageControl.scmRecoveryVmProof].filter(Boolean).map(item=>Date.parse(item.at))),
  'Host shutdown/memory evidence predates the latest stress run');
assert(pnpGuard.passed && pnpGuard.hostRefused && pnpGuard.fixtureNotCreated && !pnpGuard.kernelRuntimeTested);
assert(pnpGuard.benchmarkStressHostRefused===true);
assert(crash.recovered && crash.sha256Matches && crash.temporaryFilesReclaimed);
assert(instance.passed && startup.running && startup.debugPortClosed && startup.debugEnvironmentAbsent);
for (const [key, value] of Object.entries(installer)) if (typeof value === 'boolean') assert(value, key);
assert(cleanup.every(item => !fs.existsSync(item.Path)), 'Benchmark payload remained');
assert(!fs.existsSync(native.downloadDestination), 'Native download test folder remained');
for (const entry of fs.readdirSync(path.join(root, 'artifacts/runtime'), {withFileTypes:true})) {
  if (entry.isDirectory() && entry.name.startsWith('crash-')) {
    assert(!fs.readdirSync(path.join(root,'artifacts/runtime',entry.name)).some(name => name.endsWith('.part')));
  }
}
const rust = fs.readFileSync(path.join(root,'artifacts/cache-control-workspace-tests.log'),'utf8');
const clippy = fs.readFileSync(path.join(root,'artifacts/cache-control-clippy.log'),'utf8');
assert(/Finished `dev` profile/.test(clippy) && !/error(?:\[|:)/.test(clippy),'Clippy evidence did not pass');
assert(!/test result: FAILED/.test(rust));
const rustPassed = [...rust.matchAll(/test result: ok\. (\d+) passed/g)].reduce((sum,match)=>sum+Number(match[1]),0);
assert.equal(rustPassed,96);
assert.equal(environment.activeCacheDrivers.length,0);
assert.equal(environment.kernelSignature,'NotSigned');
const buildTime=fs.statSync(path.join(root,'target/release/SuperHDD.exe')).mtimeMs;
for(const evidence of [native,installer,startup,instance])assert(Date.parse(evidence.at)>=buildTime,'Runtime evidence predates the release binary');
const hash = file => ({Path:path.join(root,file),Hash:crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex').toUpperCase()});
assert.equal(startup.sha256,hash('target/release/SuperHDD.exe').Hash);
assert(driverBuild.passed && driverBuild.binaries.length===8);
for(const item of driverBuild.binaries) assert.equal(hash(path.relative(root,item.path)).Hash,item.sha256,'Driver artifact changed after checked build');
for(const item of driverBuild.sourceHashes) assert.equal(hash(path.relative(root,item.Path)).Hash,item.Hash,'Driver source changed after checked build');
const previousHashes=read('artifacts/release-sha256.json');
const artifactFiles=[...previousHashes.map(item=>path.relative(root,item.Path)),
  'artifacts/driver-raw-safe/SuperHDDCacheDriver.sys','artifacts/driver-raw-lab/SuperHDDCacheDriver.sys',
  'artifacts/driver-raw-safe-analysis/SuperHDDCacheDriver.sys','artifacts/driver-raw-lab-analysis/SuperHDDCacheDriver.sys',
  'artifacts/driver-pnp-safe-analysis/SuperHDDCacheDriver.sys','artifacts/driver-pnp-lab-analysis/SuperHDDCacheDriver.sys',
  'target/release/examples/raw_lab.exe','target/release/examples/raw_benchmark.exe',
  'target/release/examples/native_bridge_probe.exe','target/release/examples/managed_session_probe.exe','target/release/examples/scm_recovery_probe.exe','target/release/examples/container_probe.exe','target/release/examples/container_lifecycle_probe.exe'];
const artifactHashes=[...new Set(artifactFiles)].map(hash);
const unsignedLabHash=hash('artifacts/driver-raw-lab/SuperHDDCacheDriver.sys').Hash;
const signedLabHash=hash('artifacts/runtime/kernel-lab/signed-driver.sys').Hash;
const historicalKernelTools=read('artifacts/kernel-tool-baseline.json').tools.map(tool=>{
  const file=path.join('artifacts/runtime/kernel-lab/historical-tools',tool.name);
  assert.equal(hash(file).Hash,tool.sha256,'Historical kernel tool archive mismatch');
  return {...tool,path:file,currentBuildMatches:hash(path.join('target/release/examples',tool.name)).Hash===tool.sha256};
});
const executedToolHash=name=>historicalKernelTools.find(tool=>tool.name===name)?.sha256;
assert.equal(storageControl.probeBinarySha256,hash('target/release/examples/container_probe.exe').Hash);
assert.equal(storageControl.kernelSha256,unsignedLabHash);
for(const item of storageControl.sourceHashes) assert.equal(hash(path.relative(root,item.Path)).Hash,item.Hash,'Container source changed after proof');
assert.equal(bridgeGuard.probeSha256,hash('target/release/examples/native_bridge_probe.exe').Hash);
assert.equal(bridge.preparation.driverHash,signedLabHash);
assert.equal(bridge.preparation.signature,'Valid');
for(const [name,file] of Object.entries({'SuperHDDService.exe':'target/release/SuperHDDService.exe','native_bridge_probe.exe':'target/release/examples/native_bridge_probe.exe'})) {
  assert(bridge.binaryHashes.some(item=>item.name===name && item.sha256===hash(file).Hash),'Native bridge binary changed after VM proof');
}
for(const file of ['crates/platform/src/driver.rs','crates/platform/src/raw_lab.rs','crates/platform/src/ipc.rs','crates/platform/src/discover.ps1','services/superhdd-service/src/main.rs','crates/platform/examples/native_bridge_probe.rs']) {
  const current=hash(file); assert(bridge.sourceHashes.some(item=>item.Path===current.Path && item.Hash===current.Hash),'Native bridge source changed after VM proof');
}
assert(bridge.probe.serviceAfter.hits>bridge.probe.serviceBefore.hits && bridge.probe.serviceAfter.ssdReadBytes>bridge.probe.serviceBefore.ssdReadBytes);
assert(bridge.probe.disabled.attached===false && bridge.probe.disabled.nativeVolume.sourceDiskNumber===null && bridge.probe.disabled.nativeVolume.backingDiskNumber===null);
assert.equal(productionSignature.driverSha256,unsignedLabHash);
assert(productionSignature.authenticodeStatus==='NotSigned' && productionSignature.productionSignatureAccepted===false && productionSignature.windowsKernelPolicyVerifyExitCode!==0);
const shippingFiles=['crates/platform/src/lib.rs','crates/platform/src/discover.ps1','apps/desktop/src/App.tsx','apps/desktop/src/api.ts','apps/desktop/src/types.ts','apps/desktop/src/locales/en.json','apps/desktop/src/locales/ar.json'];
for(const file of shippingFiles) {
  // A legacy unknown JSON key in a Rust test fixture is not compiled into shipping code.
  const source=fs.readFileSync(path.join(root,file),'utf8').split('#[cfg(test)]')[0];
  assert(!/primocache|primo_path|primoPath|primo_inspect/i.test(source), 'Vendor runtime/discovery remained in '+file);
}
const nativeBridge={readOnlyV4TelemetryConnected:true,privilegedDesktopMutationExposed:false,vendorRuntimeUsed:false,vendorDiscoveryRemoved:true,
  wireAbiChanged:false,hostRefusalPassed:true,positiveServiceVmProbePassed:true,
  deployedServiceVmProof:{at:bridge.at,runId:bridge.runId,checks:bridge.probe.checks,readOperations:bridge.probe.readOperations,readBytes:bridge.probe.readBytes,readSha256:bridge.probe.readSha256,
    ssdHitsDelta:bridge.probe.serviceAfter.hits-bridge.probe.serviceBefore.hits,ssdReadBytesDelta:bridge.probe.serviceAfter.ssdReadBytes-bridge.probe.serviceBefore.ssdReadBytes,
    fixtureRemoved:bridge.probe.fixtureRemoved,cleanup:bridge.cleanup,binaryHashes:bridge.binaryHashes,sourceHashes:bridge.sourceHashes},
  priorCanceledAttempt:priorBridgeAttempt,latestHostObservation:bridgeHost,labFinalHost:finalHost,
  scope:'The current service and probe passed the native-v4 read-only desktop contract inside the isolated VM, including ordinary byte-checked reads, real SSD-hit deltas and detached/null identities. This does not activate a physical Windows-wide cache.'};
function verifyStress(result, verifier, pending) {
  assert(result.passed && result.phase==='completed' && result.error===null && result.fixtureRemoved && result.cleanup.cacheDetached);
  assert.equal(result.driver.sha256,signedLabHash); assert.equal(result.driver.signature,'Valid');
  assert.equal(result.toolSha256,executedToolHash('raw_benchmark.exe'));
  const phases=result.stress.phases;
  assert(result.stress.passed && phases.length===6 && phases.every(item=>item.passed));
  for(const item of phases.slice(0,5)) {
    assert(item.counts.operations===1024 && item.counts.operations===item.counts.correctCompletions+item.counts.cancelledCompletions);
    assert(item.counts.maximumSampledDriverPending<=16 && item.after.cache.pendingReads===0);
  }
  assert(phases[0].counts.maximumSampledDriverPending>1);
  assert(phases[1].counts.cancelledCompletions>0 && phases[1].cancelledDriverReadsDelta>0);
  assert(phases.slice(2,5).every(item=>item.counts.controlsWithReadsPending>0));
  const writes=phases[5];
  assert(writes.mode==='concurrent-source-writes' && writes.writes===32 && writes.writesWithDriverReadsPending>0
    && writes.overlappingReads===1024 && writes.fullBlockChecksAfterCompletedWrite===64 && writes.fixtureRestored);
  if(verifier) {
    for(const query of [result.verifierActive.query,result.verifierAfter.query]) {
      const flags=parseInt(query.match(/Verifier Flags:\s*0x([a-fA-F0-9]+)/)?.[1]||'0',16);
      assert((flags&0x209bb)===0x209bb && /MODULE: driver\.sys/.test(query));
      if(pending) assert((flags&0x200)===0x200);
    }
    assert.equal(result.verifierActive.boot,result.verifierAfter.boot);
    assert(result.verifierReset.resetForNextBoot && /Verifier Flags:\s*0x0+\s/.test(result.verifierReset.querySettings));
  }
  return {passed:true,verifier,forcePending:pending,seconds:result.stress.seconds,
    requestsInFullComparisonPhases:5120,completedFullBlockComparisons:phases.slice(0,5).reduce((sum,item)=>sum+item.counts.correctCompletions,0)+64,
    overlappingWriteReadsWithTailComparison:1024,fullBlockChecksAfterWriteCompletion:64,
    cancelledCompletions:phases[1].counts.cancelledCompletions,
    maximumSampledDriverQueue:Math.max(...phases.slice(0,5).map(item=>item.counts.maximumSampledDriverPending)),
    activeChildCancellationObserved:phases.slice(0,5).some(item=>item.childCancellationsDelta>0),
    phases,toolSha256:result.toolSha256,signedDriverSha256:result.driver.sha256};
}
const stressResults={normal:verifyStress(cancellationStress,false,false),standardVerifier:verifyStress(verifierStress,true,false),forcedPendingVerifier:verifyStress(pendingStress,true,true)};
const probeExecution=[resumedProbe,signing].find(item=>item && item.error===null && item.phase==='completed-lab-probe'
  && item.copiedHashes?.some(file=>file.name==='pnp_probe.exe' && file.sha256===executedToolHash('pnp_probe.exe'))
  && (item===signing || (item.probe?.passed===true && item.probe?.driverImageSha256===signedLabHash
      && item.probeArtifactSha256===hash('artifacts/runtime/kernel-lab/probe.json').Hash)));
const rawRuntimePassed=kernelProbe?.passed===true && kernelProbe?.rawExtras?.passed===true
  && kernelProbe?.driverImageSha256===signedLabHash && kernelProbe?.driverSignatureStatus==='Valid'
  && signing.error===null && signing.phase==='completed-lab-probe'
  && signing.copiedHashes?.some(item=>item.name==='driver.sys' && item.sha256===unsignedLabHash)
  && Boolean(probeExecution);
const hardwarePassed=rawRuntimePassed && hardwareBenchmark?.passed===true
  && hardwareBenchmark.driver?.sha256===signedLabHash && hardwareBenchmark.driver?.signature==='Valid'
  && hardwareBenchmark.hostHddFileRemoved===true;
const hardwareHddReadsAvoided=hardwarePassed && hardwareBenchmark.phases.Warm.physicalHdd.readOperations===0;
assert(rawRuntimePassed,'Recorded probe image is not bound to a passing signed-driver execution');
assert(hardwarePassed && hardwareHddReadsAvoided,'Recorded hardware-backed lab benchmark did not pass for this signed image');
const report = {
  recordedAt: new Date().toISOString(),
  historicalKernelTools,
  evidenceAuditPassed: true,
  productionReady: false,
  transparentAccelerationActive: false,
  qualifiedKernelDriver: false,
  verifiedChecks: {
    rustTests: rustPassed, browserTests: ui.stats.expected,
    sharedDriverCAssertions: driverState.checks,
    rawStoreCAssertions: rawStore.checks, isolatedServerGuestBooted: true,
    rawKernelProbePassed: rawRuntimePassed,
    nativeV4DeployedServiceBridgePassed: true,
    nativeOwnedSsdContainerProofPassed: true,
    nativeManagedSessionVmProofPassed: true,
    nativeHardwareBackedVmBenchmarkPassed: hardwarePassed,
    physicalHddReadAvoidanceInLabWindow: hardwareHddReadsAvoided,
    overlappedCancellationAndConcurrentWriteLabPassed: true,
    standardDriverVerifierLabPassed: true,
    forcedPendingDriverVerifierLabPassed: true,
    nativeChecks: Object.keys(native.checks).length,
    nativeSecondLaunch: true, actualProcessCrashRecovery: true,
    desktopInstallStartupUninstall: true, installedAndStandaloneSha256Equal: true,
    testPayloadsReclaimed: hardwareBenchmark?.hostHddFileRemoved===true,
    normalApplicationStartupPreviouslyVerified: true,
    applicationCurrentlyRunning: bridgeHost.applicationRunning,
    auditDebugPortClosed: true
  },
  benchmarkScope: 'Native ordinary-file VM driver benchmark over host D: HDD / M: NVMe, with whole-disk physical counter windows. Earlier user-mode FileCache harness is separate. Neither is production Windows-wide or game qualification.',
  nativeHardwareBenchmark: hardwareBenchmark && {passed:hardwarePassed,physicalCounterScope:'Whole physical disk, not per-file ETW',speedRatio:hardwareBenchmark.speedRatio,phases:Object.fromEntries(Object.entries(hardwareBenchmark.phases).map(([name,phase])=>[name,{iops:phase.guest.iops,p50Milliseconds:phase.guest.p50Milliseconds,p95Milliseconds:phase.guest.p95Milliseconds,sha256:phase.guest.sha256,sourceReadBytes:phase.guest.sourceReadBytesDelta,ssdReadBytes:phase.guest.ssdReadBytesDelta,errors:phase.guest.errorsDelta,physicalHdd:phase.physicalHdd}])),payloadRemoved:hardwareBenchmark.hostHddFileRemoved===true},
  nativeBenchmarkCacheBudgetBytes: hardwareBenchmark?.cacheBudgetBytes,
  nativeStress:stressResults,
  isolatedLabVhdStorageRetained: true,
  benchmarkBackingPeakBytes: cleanup.filter(item=>item.Path.startsWith('M:')).reduce((sum,item)=>sum+item.Bytes,0),
  remainingBenchmarkBackingBytes: 0,
  dedicatedRamCacheBytes: 0,
  nativeBridge,
  storageControl,
  signingPrerequisites:{userConfirmedNoHardwareDeveloperAccount:true,userConfirmedNoEvCertificate:true,
    budget:'free-only',freeMicrosoftKernelSigningRouteConfirmed:false,
    ossApplicationSigningCandidate:'SignPath Foundation',ossKernelSubmissionCoverageConfirmed:false,
    sponsorAccepted:false,sourcePublished:false,licenseChanged:false,
    individualEvRouteIdentified:true,paidRouteSelected:false,iraqIssuanceConfirmed:false,
    publisherRegistrationConfirmed:false,externalInquirySent:false,purchaseMade:false,
    brief:'docs/release/free-signing-research.md',historicalPaidBrief:'docs/release/signing-provider-brief.md'},
  productionSignatureCheck: productionSignature,
  isolatedDevelopmentVmConfiguredMemoryBytes: provision.memoryBytes,
  isolatedDevelopmentVmAssignedMemoryBytes: finalHost.assignedVmMemoryBytes,
  finalHostState: finalHost,
  blockers: [
    'The current native driver deliberately accepts disposable file-backed virtual volumes only; physical-source support, controller and deployment are not production-qualified.',
    'Native RAW initialization, managed-service lifecycle and protected LocalSystem SCM process-crash restart/reconciliation passed on isolated virtual parents. Power-loss/cold-boot qualification and production desktop activation remain pending.',
    'The publisher has no Hardware Developer account or EV certificate and requires a free solution. No free Microsoft kernel-driver signing sponsor or submission route is confirmed; conditional OSS application signing alone is insufficient.',
    'Bounded VM concurrency, queue cancellation, concurrent writes and short Verifier runs passed; active-child cancellation, long-duration/fault, PnP/power/removal and low-memory reliability remain unqualified.',
    ...(!rawRuntimePassed ? ['The signed isolated-VM RAW driver probe has not passed. Last lab error: '+(signing.error || signing.phase || 'pending')] : []),
    'Physical-device ancestry, recursion rejection, online kernel resize and cold-boot/power-loss qualification remain pending.',
    'No qualified Microsoft-signed production driver has been installed or activated on physical user volumes.',
    ...(!hardwareHddReadsAvoided ? ['A passing warm-cache HDD-I/O-avoidance benchmark remains pending.'] : []),
    'Per-file physical storage tracing and Windows 10/11 hardware qualification remain pending.',
    'Production service/driver installation, rollback and game-loading benchmarks remain pending.'
  ],
  hostEvidence: { restrictedShellElevated:false, hypervVmCreated:true, isolatedGuestBooted:true, hostSecurityChanged:false, priorUacCancellation:uac.canceledByWindows, activeCacheDrivers: [] },
  experimentalDriver: {protocolVersion:2,maximumQueuedReads:16,maximumReadBytes:1048576,rawVhdOnly:true,runtimeQualified:false},
  pnpLabCandidate: {protocolVersion:3,mountedNtfsSourceImplemented:true,queuedCancellationImplemented:true,activeChildCancellationImplemented:true,physicalSourceAllowed:false,pagingCacheHits:false,persistentReuse:false,signatureStatus:pnpLab.signatureStatus,hostRefusalVerified:pnpGuard.passed,kernelRuntimeTested:false,connectedToDesktop:false},
  rawLabCandidate: {protocolVersion:4,lockedRawVolumeBackendImplemented:true,pagingReadSupportImplemented:true,persistentIndexImplemented:true,targetedWriteInvalidationImplemented:true,sourceRevalidationMandatory:true,physicalSourceAllowed:false,hostRefusalVerified:pnpGuard.rawHostRefused===true,guestDriverTestSigned:signing.guest?.signed===true,kernelProbePassed:rawRuntimePassed,coldBootPersistenceQualified:false,connectedToDesktop:true,desktopConnectionScope:'Read-only telemetry; positive deployed-service VM probe passed',desktopActivationExposed:false},
  artifacts: artifactHashes,
  report: 'docs/release/2026-10-19-scm-recovery.md'
};
const sourceFiles=['drivers/superhdd-cache-driver/pnp_driver.c','drivers/superhdd-cache-driver/raw_backend.inc','drivers/superhdd-cache-driver/raw_store.h','drivers/superhdd-cache-driver/pnp_protocol.h','drivers/superhdd-cache-driver/cache_state.h','drivers/superhdd-cache-driver/cache_invalidation.h','crates/platform/src/raw_lab.rs','crates/platform/examples/raw_lab.rs','crates/platform/examples/pnp_probe.rs','crates/platform/examples/raw_benchmark.rs','crates/platform/examples/raw_stress/mod.rs','crates/platform/tests/download_integration.rs','tests/driver/raw_store_test.c','tests/driver/cache_state_test.c','scripts/build-driver.ps1','scripts/build-lab-tools.ps1','scripts/prepare-kernel-lab.ps1','scripts/start-kernel-lab-recovery.ps1','scripts/invoke-kernel-lab.ps1','scripts/kernel-lab-guest.ps1','scripts/invoke-hardware-benchmark.ps1','scripts/kernel-hardware-guest.ps1','scripts/invoke-cancellation-stress.ps1'];
sourceFiles.push(...shippingFiles,'crates/platform/src/driver.rs','crates/platform/src/ipc.rs','crates/platform/examples/native_bridge_probe.rs','scripts/test-native-bridge.ps1','scripts/test-native-release.ps1','scripts/test-native-ui.cjs','scripts/test-installer.ps1','tests/ui/desktop.spec.ts');
sourceFiles.push('Cargo.toml','Cargo.lock','crates/storage-control/Cargo.toml','crates/storage-control/src/lib.rs','crates/storage-control/src/windows.rs','crates/storage-control/examples/container_probe.rs','scripts/test-cache-container.ps1','scripts/record-storage-control-audit.cjs');
sourceFiles.push('crates/storage-control/src/journal.rs','crates/storage-control/src/format.rs','crates/storage-control/src/raw_volume.rs','crates/storage-control/src/session.rs','crates/storage-control/examples/container_lifecycle_probe.rs','crates/platform/src/managed_cache.rs','services/superhdd-service/src/lab_control.rs','scripts/test-cache-lifecycle.ps1','scripts/test-cache-session-host-guard.ps1','scripts/test-native-bridge-host-guard.ps1','scripts/verify-cache-control.ps1','artifacts/kernel-tool-baseline.json');
sourceFiles.push('crates/storage-control/src/lab.rs','crates/storage-control/src/lab_windows.rs','crates/platform/examples/managed_session_probe.rs','scripts/test-managed-session.ps1');
sourceFiles.push('crates/storage-control/src/protected.rs','crates/storage-control/src/recovery.rs','services/superhdd-service/src/main.rs','services/superhdd-service/src/managed_host.rs','services/superhdd-service/Cargo.toml','crates/platform/Cargo.toml','crates/platform/examples/scm_recovery_probe.rs');
const evidence={recordedAt:report.recordedAt,productionReady:false,transparentAccelerationActive:false,sourceChecks:{rustTests:rustPassed,sharedCAssertions:driverState.checks,rawStoreCAssertions:rawStore.checks,rustMarkerDecodedByC:rawStore.rustMarkerDecoded===true,clippyPassed:true,rawSafeAndLabBuildAndStaticAnalysisPassed:true,hostRefusalVerified:pnpGuard.rawHostRefused===true},lab:{guestBooted:true,os:guest.guest.os,build:guest.guest.build,generation:provision.generation,networkAdapters:provision.networkAdapters,memoryBytes:provision.memoryBytes,signingAction:signing,signedImageSha256:signedLabHash,unsignedImageSha256:unsignedLabHash,kernelProbe,hardwareBenchmark},kernelRuntimeTested:rawRuntimePassed,ordinaryFilePositiveProbeRun:rawRuntimePassed,hardwareBenchmarkPassed:hardwarePassed,physicalHddReadAvoidanceInLabWindow:hardwareHddReadsAvoided,sourceHashes:sourceFiles.map(hash),artifactHashes,blockers:report.blockers};
fs.writeFileSync(path.join(root,'artifacts/release-sha256.json'),JSON.stringify(artifactHashes,null,2)+'\n');
fs.writeFileSync(path.join(root,'artifacts/release-readiness.json'),JSON.stringify(report,null,2)+'\n');
evidence.lab.stress=stressResults;
evidence.lab.probeExecution=probeExecution;
evidence.nativeBridge=nativeBridge;
evidence.storageControl=storageControl;
fs.writeFileSync(path.join(root,'artifacts/raw-source-evidence.json'),JSON.stringify(evidence,null,2)+'\n');
console.log(JSON.stringify({...report,artifacts:report.artifacts.length},null,2));
