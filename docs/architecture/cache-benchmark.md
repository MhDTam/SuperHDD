# Storage benchmark

The desktop's **Performance & diagnostics → Storage benchmark** runs an automated, owned-fixture A/B test. It does not time a game, measure FPS, certify anti-cheat compatibility, or establish acceleration on other computers. A valid result can show a slowdown or an inconclusive difference. No report is uploaded automatically.

## Procedure

Choose an existing HDD/SSD configuration, close games, avoid competing disk work, then choose Standard and a workload. Quick is exploratory only. The tool creates a new deterministic high-entropy archive on each selected drive and a separate 128 MiB cache. It never moves or benchmarks original game files, empties game caches, changes boot settings, or purges system RAM caches.

| Profile | Archive on each drive | Requests per phase | Paired rounds | Selected 64 KiB blocks |
| --- | --- | --- | --- | --- |
| Quick | 64 MiB | 128 | 2 | 8 MiB |
| Standard | 256 MiB | 512 | 6 | 32 MiB |

All measured arms use the same single-threaded, queue-depth-one client, seed 104729 and unique offsets `((operation * 509 + seed) % archiveBlocks) * 65536`. Mixed uses 25% 4 KiB / 75% 64 KiB requests; Small uses 4 KiB; Large uses 64 KiB. These are declared synthetic address patterns, not captured game traces or whole-cache-capacity tests. The client validates every returned byte against a position-dependent pattern. Raw request latencies exclude verification; whole-phase wall time includes identical verification work.

The direct HDD and first cache passes are saved. Repeated 64 KiB training passes use the normal second-touch/burst admission algorithm. Alternate training rotates the same block set by half its order, so the policy's initial low-density prefix can be revisited later in a burst. Training is disclosed, outside the warm speed estimate, and includes its own raw samples. Deliberate waits and fixture creation are excluded from phase timings; full-run UTC timestamps remain available. Validation requires at least 95% of payload served from SSD; insufficient warming invalidates the run rather than manufacturing a score.

Six pairs alternate direct→cached and cached→direct. Each pair requests identical addresses and sizes. Independent references read the same pattern directly from the SSD archive and from a pre-read, Windows-buffered HDD archive. Those are single reference readings, not confidence estimates. A warm Windows cache can outperform the engine, and storage speed ratios must not be presented as game-loading gains.

## Windows caching and attribution

The test builds `worker.cpp` and `pool-v2.h` with `SUPERHDD_STORAGE_BENCHMARK`. The direct client, benchmark worker source handle, and pool reader use aligned `FILE_FLAG_NO_BUFFERING`; cache writes use write-through. Ordinary game workers retain their default buffering and are not replaced. This **storage-isolated engine variant** controls a common RAM-cache confound but is not a measurement of the complete production/game path. Pool metadata remains memory mapped. Disk/controller caches, antivirus and other background activity are not eliminated. No claim about physical hardware I/O counters is made.

Counters identify the private probe executable and originating process. Windows can issue additional callbacks on a probe-owned file handle. The report retains their bytes and conservatively subtracts *all* excess callback bytes from SSD attribution before checking the 95% threshold; additional callbacks never inflate the claimed SSD payload. Counter coverage must include the probe's bytes, source plus SSD accounting must balance, and direct phases must not trigger probe-attributed cache reads.

## Statistics and evidence

For each pair the ratio is direct wall time / cached wall time. The aggregate is the geometric mean of paired ratios. An approximate 95% Student t interval is calculated on log ratios (5 degrees of freedom for Standard; 1 for Quick). An interval above 1 means faster for this workload, below 1 means slower, and containing 1 is inconclusive. Repeated rounds within a session are not independent hardware samples. Restart/repeat sessions and real game-loading measurements are needed for broader publication claims.

Reports retain every pair, first pass, training cost, hardware models, OS, parameters, build/source SHA-256 hashes, source snapshots and raw samples. Byte correctness, operation counts, raw address equality, SSD attribution and cleanup must pass. Failed/cancelled runs remain visible without a speed claim. `publicationReady` means a complete valid six-pair storage report, **not** product qualification or permission to claim game speedups. SHA-256 manifests detect package changes; they are not a cryptographic publisher signature.

Use **Open full report** or **Open shareable ZIP**. The ZIP contains HTML, JSON, CSV, sources and a manifest. It excludes local fixture paths, account names, SIDs and disk serial numbers. Review the report before publishing it.

## Reproduction and recovery

Prerequisites: Windows with NTFS HDD/SSD on separate physical devices, installed Dokan 2.3.1, MSVC C++ tools, Windows SDK 10.0.26100.0. Build from the repository root:

```powershell
& scripts/build-cache-benchmark.ps1
# In a built desktop checkout, the same native job can be started with:
& target/release/examples/benchmark_client.exe start CONFIGURATION_ID standard mixed
& target/release/examples/benchmark_client.exe status
```

Valid workloads are `mixed`, `small`, `large`. Configuration IDs are existing entries, not arbitrary paths. Build scripts and six C++ source/header files are frozen into each successful-start report package. The local runner creates `SuperHDD-Benchmark-<runId>` roots on the selected drives with ownership markers. Cleanup verifies volume identities, exact paths and mount tags, then removes only known fixture leaves. It never recursively deletes a tree. Cancellation stops only its private probe and worker; an orphaned worker watches its controller and exits. **Recover test files** verifies recorded process creation identities and safely removes an interrupted run's owned fixture. Unexpected files, mounts or ownership changes are preserved for inspection.

Microsoft reference: [File buffering](https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering).
