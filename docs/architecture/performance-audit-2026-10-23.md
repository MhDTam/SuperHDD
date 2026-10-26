# Performance and correctness audit: 2026-10-23

This audit covers the current selected-folder Dokan backend, its shared cache,
native configuration manager, telemetry/history, desktop controls and recovery.
The historical unsigned-driver experiments were covered by their existing Rust
validation tests; no such driver was loaded or qualified by this audit.

## Changes

1. **Move SSD reads and SHA-256 outside the pool mutex.** A reader copies the
   record identity, checksum, serial and pool epoch under a short try-lock. It
   then performs an explicitly offset overlapped read on a separate handle and
   authenticates the copied payload. Concurrent eviction can cause a safe miss;
   it cannot return another key's bytes. A final try-lock touches LRU only when
   key, serial, epoch and index bounds still match. A stale reader cannot modify
   a reused slot or resurrect an abandoned index. Expected reuse races are
   counted separately from actual stable-record corruption.
2. **Maintain file-level inventory incrementally.** The previous snapshot walked
   and copied every resident block every 500 ms while holding the shared mutex.
   Format 2 maintains resident bytes and slot counts per source file during put,
   eviction, invalidation and resize. Snapshot work scales with cached file
   count rather than block count. The file table is bounded at 8,192 entries;
   current active mounts can hold at most 8 × 512 read leases. Metadata remains
   below the existing 64 MiB free-space allowance, enforced by a static assertion.
3. **Make periodic snapshots nonblocking.** A busy pool skips a periodic update
   and tries again on the next tick; telemetry does not queue behind an SSD fill
   or fail the worker just because another owner is using the pool.
4. **Enable production C++ optimization.** The worker build previously omitted
   `/O2`. Worker and test binaries now use `/O2`, while retaining debug symbols
   and `/W4 /WX`. The isolated pool comparison builds *both* implementations
   with `/O2` so its architectural comparison does not depend on that flag.
5. **Keep native telemetry off the desktop thread.** The Tauri telemetry command
   now dispatches its blocking native/IPC work through `spawn_blocking`, matching
   the other runtime commands. Its IPC response shape is unchanged.
6. **Count bypassed reads as misses.** Paused, writable and otherwise unleased
   source reads were absent from block-miss counters. They now contribute to
   misses and bypass counts, keeping the displayed hit ratio meaningful.

Clippy findings were also removed by moving a test module below production
items and removing an unnecessary return. The manager regression now preserves
the actual initial profile set instead of requiring a historical Dragon Ball
profile that the user had already removed. The first manager run's false
failure is retained separately in the evidence directory.

## Format migration

The source files and shared cache budget are unaffected by the index change.
Format 2 refuses to reinterpret an incompatible index while an older worker
still has the pool open. Deployment stops existing mounts through the native
manager while games are closed, preserves their profiles/configurations and
starts a new pool lifetime. Disposable old cached bytes are discarded. It does
not modify the Windows boot configuration, pagefile or installed Dokan driver.

## Validation and evidence

Evidence is under `artifacts/audit-performance`, with integration proofs under
`artifacts/loading-cache`. The final `qualified.json` binds the deployed worker
to exact binary hashes and the current filesystem, session and manager runs.

- All 124 Rust workspace tests passed in the normal user context. Four named-pipe
  tests initially received Access Denied from the restricted runner; they passed
  outside that restriction without an administrator token.
- All 40 desktop/browser regression tests passed, including Arabic layout,
  independent configurations, cache clearing, numeric input, history and runtime
  state controls. UI behavior and IPC response shapes remain compatible.
- Clippy passed for every workspace target with warnings treated as errors.
- C++ pool tests validate ownership, shared capacity, LRU, CRC rejection, partial
  ranges, concurrent operations, abandoned locks, forced slot reuse during a
  read, reset during an in-flight read and per-file inventory after resize/clear.
- The 13-check filesystem suite covers named streams, mapped writes, EOF,
  mutation, source writer exclusion, pause, clear, normal/crash restoration and
  the new bypass-miss assertion.
- The 12-check session suite covers shared capacity, independent SSDs, owner
  clearing, resize, write invalidation and worker/supervisor recovery.
- Native manager tests cover device identity, duplicate sources, independent
  configuration changes, original profile retention and restoration.

The final reports contain exact measured numbers and run identifiers; do not
substitute the preliminary pool or source-read runs for those final results.
Physical performance comparisons run sequentially after builds and integration
tests finish. The synthetic resident-cache benchmark uses 2,048 resident blocks,
eight reader threads and an explicit 1 ms sleep on a cache miss; it is a measure
of cache contention, not game FPS or physical HDD throughput. The source-path
fixture warms the original file in Windows cache to isolate callback overhead.

## Remaining measurement limits

- No matched game loading-time or frame-time comparison was performed by this
  audit. The user's positive game experience is useful but is not converted to
  a numeric speedup. The earlier HDD-only capture remains an uncontrolled
  baseline with its own documented timestamp/display-latency limitations.
- Loading priority remains a second-touch/burst heuristic, not a visual loading
  screen detector. First-touch reads may not benefit; recurring gameplay reads
  may qualify. No manual training requirement was introduced.
- A small hit still verifies the whole resident range (up to 64 KiB). The new
  `cacheReadApiBytes` counter makes this extra SSD-file read traffic observable.
- Write durability and source invalidation are deliberately retained. Parent
  path validation and metadata callbacks still have shared synchronization.
- Cached bytes do not survive a fresh pool lifetime after reboot. Recovery
  remains journal-bound; this audit does not add Windows startup tasks.
