// Current stress evidence can be recorded even when local Windows elevation
// prevents final VM cleanup. Never turn a historical Off snapshot into current proof.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const root = path.resolve(__dirname, '..');
const folder = path.join(root, 'artifacts/runtime/kernel-lab');
const read = name => JSON.parse(fs.readFileSync(path.join(folder, name), 'utf8').replace(/^\uFEFF/, ''));
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').toUpperCase();
const toolHash = sha(path.join(root, 'target/release/examples/raw_benchmark.exe'));
const driverHash = sha(path.join(folder, 'signed-driver.sys'));
const sources = ['crates/platform/examples/raw_benchmark.rs', 'crates/platform/examples/raw_stress/mod.rs'];
const normal = read('cancellation-stress.json');
const results = [];
for (const [name, pending] of [['verifier-stress.json', false], ['verifier-pending-stress.json', true]]) {
  const report = read(name);
  assert(report.passed && report.phase === 'completed' && report.error === null && report.fixtureRemoved && report.cleanup.cacheDetached);
  assert.equal(report.toolSha256, toolHash);
  assert.equal(report.driver.sha256, driverHash);
  assert.equal(report.driver.signature, 'Valid');
  for (const source of sources) {
    const full = path.join(root, source);
    assert(report.sourceHashes.some(item => item.Path === full && item.Hash === sha(full)));
  }
  const phases = report.stress.phases;
  assert(report.stress.passed && phases.length === 6 && phases.every(item => item.passed));
  for (const item of phases.slice(0, 5)) {
    assert(item.counts.operations === 1024 && item.counts.operations === item.counts.correctCompletions + item.counts.cancelledCompletions);
    assert(item.counts.maximumSampledDriverPending <= 16 && item.after.cache.pendingReads === 0);
    assert.equal(item.after.cache.errors, item.before.cache.errors);
  }
  assert(phases[0].counts.maximumSampledDriverPending > 1 && phases[1].cancelledDriverReadsDelta > 0);
  assert(phases.slice(2, 5).every(item => item.counts.controlsWithReadsPending > 0));
  const writes = phases[5];
  assert(writes.writes === 32 && writes.writesWithDriverReadsPending > 0 && writes.overlappingReads === 1024
    && writes.fullBlockChecksAfterCompletedWrite === 64 && writes.fixtureRestored);
  for (const query of [report.verifierActive.query, report.verifierAfter.query]) {
    const flags = parseInt(query.match(/Verifier Flags:\s*0x([a-fA-F0-9]+)/)?.[1] || '0', 16);
    assert((flags & 0x209bb) === 0x209bb && /MODULE: driver\.sys/.test(query));
    if (pending) assert((flags & 0x200) === 0x200);
  }
  assert.equal(report.verifierActive.boot, report.verifierAfter.boot);
  assert(report.verifierReset.resetForNextBoot && /Verifier Flags:\s*0x0+\s/.test(report.verifierReset.querySettings));
  results.push({report: name, at: report.at, passed: true, forcePending: pending, seconds: report.stress.seconds,
    cancelledRequests: phases[1].counts.cancelledCompletions, postWriteFullBlockChecks: 64,
    activeChildCancellationObserved: phases.slice(0, 5).some(item => item.childCancellationsDelta > 0)});
}
const finalHost = read('final-host-state.json');
const latest = Math.max(Date.parse(normal.at), ...results.map(item => Date.parse(item.at)));
const finalHostCurrent = Date.parse(finalHost.at) >= latest;
const result = {at: new Date().toISOString(), passed: true, productionReady: false, transparentAccelerationActive: false,
  toolSha256: toolHash, signedDriverSha256: driverHash, profiles: results,
  normalRunMatchesCurrentTool: normal.toolSha256 === toolHash && normal.stress.phases.length === 6,
  finalHostSnapshotCurrent: finalHostCurrent,
  finalHost: finalHostCurrent ? finalHost : null,
  finalizationPending: !finalHostCurrent || normal.toolSha256 !== toolHash,
  limitations: ['Short isolated Server VM runs only.', 'No observed active-child cancellations or deliberate allocation failures.',
    'No power/removal, long-duration, Windows 10/11 or production signing qualification.']};
fs.writeFileSync(path.join(folder, 'stress-verification.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
