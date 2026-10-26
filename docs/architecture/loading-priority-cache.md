# Shared loading-priority cache

The desktop uses `tools/loading-cache` rather than the earlier fixed-share
`config-cache` worker. Existing storage configurations and game profiles remain.

Each configuration owns one sparse SSD data file and one shared metadata index.
All enabled games on that configuration use the same capacity. The key includes
the game owner, source volume, file identity and block offset. A global LRU list
replaces the least recently accessed resident block when the pool is full.
Clearing a game invalidates only that owner's records. Stopping a configuration
restores its folders and releases its owned pool files.
Space checks credit actual sparse-file allocation rather than logical file
length, and allow 64 MiB of index overhead per configured pool.

The admission policy (revised 2026-10-26, see the end of this document):

- While the pool has free space, every miss is filled. Nothing is displaced,
  so the first loading pass already populates the SSD.
- Once the pool is full, non-sequential reads still enter it and replace the
  least recently used blocks. Sequential runs beyond 8 MiB on one handle are
  scan-resistant: they enter only on a second touch at least 500 ms after the
  first, within 15 minutes, during at least 1 MiB of recent read traffic.
- Probation retains at most 65,536 keys; the asynchronous fill queue holds at
  most 64 MiB of transient data. Source read leases are capped at 512 per worker.

This is still not a detector of a game's visual loading screen.

Foreground reads do not wait for SSD fills. Uncached adjacent ranges use one
buffered source read rather than forced aligned 64 KiB source rereads. Partial
cache records cover only bytes actually requested. Read callbacks release the
global context lock before I/O; contexts remain alive through shared ownership.
Background fills check the file generation and clear epoch before committing.
Writes remain synchronous to the HDD and invalidate cached/pending data first.
Checksums reject damaged cache records and fall back to the source.

Shared index access uses a process mutex. An abandoned mutex (a process died
while changing the index) discards the metadata. Since format 3, resident data
survives worker restarts, stopping a game's cache and reboots: the first process
of a fresh pool lifetime rebuilds the index links from the self-contained
records instead of discarding them. Each record's bytes stay SHA-256
authenticated. Each file also stores a validator (size, last-write time and
NTFS change time) captured while its lease excluded writers. Before a file's
records are used again, its new lease compares the validator and drops all of
its records on any difference, so a file updated by Steam or edited while its
cache was stopped is reread from the HDD.

The index uses coherent views of a local mapped file, as described by
[Microsoft's file mapping documentation](https://learn.microsoft.com/en-us/windows/win32/memory/file-mapping).
Only index metadata is mapped; SSD data is not a dedicated RAM cache.

Verification evidence is under `artifacts/loading-cache`:

- `unit-proof.log`: cross-process sharing, global LRU, independent clearing,
  resizing, partial ranges, checksum rejection, abandoned mutex recovery,
  concurrent access and admission bounds.
- `runs/389d16d5944d4ab7b9d94dfe1ef1e0d5/result.json`: 13 filesystem checks,
  including mapped writes, streams, mutation and process-crash restoration.
- `session-proof.json`: 12 checks across two mounts sharing M and an independent
  E/F pool, including worker/host recovery.
- `manager-proof.json`: 12 native configuration checks. This ran on the same
  pool protocol before the final lease-limit, diagnostic-counter and sparse
  free-space-accounting changes.
- `live-sharing-proof.json`: the final native client added a disposable fixture
  beside the active Euro Truck cache. Both referred to one 25 GB pool; clearing
  and stopping the fixture preserved Euro Truck's resident data and capacity.
- `euro-read-proof.json`: three identical 1 MiB reads of Euro Truck's `base.scs`,
  SSD hits, unchanged metadata for 132 original files and unchanged launcher
  hash. The observed read times were 70.57, 16.57 and 2.79 ms. These samples are
  not a game loading-time or frame-time benchmark and include OS cache effects.

Qualification records bind the integration proofs to worker/script hashes.
Actual game loading time and frame-time improvement require a game run.

## Euro Truck latency investigation (2026-10-23)

The saved game-worker snapshot reported 290,294 read callbacks, a 5 ms p95
histogram bound and a 1,077,765 microsecond maximum callback. About 2.69 GB came
from the source and 0.47 GB from the SSD. There were no reported checksum or
cache errors. These are cumulative I/O counters, not frame times or proof of
the cause of every game spike. At investigation time both recorded worker and
supervisor were gone; the stale mount was safely restored to the original game
directory. Euro Truck remains stopped in the cache library for a baseline run.

The read path held a per-context mutex across source I/O and repeatedly queried
file length. Read-only source leases now use an overlapped handle with explicit
offsets, allowing simultaneous reads of the same archive. File length is captured
while the lease excludes writers. A short mutex only updates sequential-admission
metadata. Stream, paused and writable-handle paths retain their synchronous
fallback. Failed lease attempts are limited to once per second per file.
Lease invalidation still waits for outstanding readers and revokes resident and
queued data before a source mutation. SSD layout, admission policy, shared LRU,
durable writes and boot settings are unchanged.

The synthetic comparison used 1,024 validated requests, eight threads sharing
one handle, and three runs each at 4 KiB and 64 KiB. The HDD source was warmed
in Windows' file cache to isolate callback overhead. At 4 KiB the previous
p95 was approximately 0.17–0.25 ms, versus 0.09–0.10 ms for the candidate. The
64 KiB throughput did not improve. This does not establish a game FPS or
loading-time improvement, nor does it bound physical HDD seek delays.

The final worker hash is bound to these proofs in `qualified.json`:

- `latency-331af1d29bec4ad5853bab7d28744bb8/result.json`: byte validation and
  concurrent source reads, including three comparisons per request size.
- `runs/e4a4be766df04916b7debdbb9780892c/result.json`: 13 filesystem and
  restoration checks, including mutation, streams and process failure.
- `session-proof.json`, run `17d113f4d79f451cb21809d31b796ff1`: 12 shared
  pool, independent-device and recovery checks.

Source-level parallelism does not remove Dokan's userspace round trips or
synchronous SSD/checksum work for cache hits. A game comparison with frame-time
capture is still needed before declaring the reported heavy lag resolved.

## First-load fill, persistence and lock fixes (2026-10-26)

Live counters from real sessions showed the second-touch policy kept most game
reads on the HDD: Euro Truck served about 15% of read bytes from the SSD
(300,809 admission bypasses), and the PUBG data-only cache about 2% (58 MB
versus 2.94 GB from the HDD, plus 15,462 lock bypasses and 96 queue drops). The
earlier admit-on-miss engine had served about 87% for Euro Truck. Restarting a
session, stopping it or rebooting also discarded every cached byte.

Changes:

1. **Admission** as described above: fill free space on first touch; once
   full, LRU for non-sequential reads, second touch only for long sequential
   runs. The fill queue grew from 4 MiB to 64 MiB.
2. **No lock-bypass misses.** A put hashed and wrote the SSD while holding the
   cross-process pool mutex, and readers that found the mutex busy went to the
   HDD. A put now hashes first, reserves its slot under the lock, writes outside
   it through an overlapped handle with an explicit offset, and publishes under
   the lock only if the epoch and reservation are unchanged. Readers wait up to
   5 ms for the index instead of treating a busy lock as a miss.
3. **Cheap write invalidation.** Invalidating one file walked the whole LRU list
   (up to 381,469 records) under the pool mutex on every write callback, even
   for files with nothing cached. It now returns immediately for such files and
   stops at the file's last resident slot otherwise.
4. **Persistence** (index format 3) with per-file validators and link rebuild,
   as described above. A full pool still evicts by LRU, and the rebuild keeps
   the recorded LRU order when the list is intact. Stopping a game keeps its
   data; Clear, removing the profile or stopping the configuration discards it.
5. **Resume after sign-in.** The logon recovery task restores interrupted
   folders, then restarts caches that were active at shutdown (each start runs
   the normal session checks, including that the game is closed). Stopping a
   game's cache removes its resume marker.

A format-2 index is discarded once on first open by a format-3 worker. A
format-3 worker refuses to share a pool with a still-running format-2 worker.

The loading replay in `docs/benchmarks/2026-10-26-loading-replay.md` compares
the previous and the new engine on identical disposable files.

