# Automated selected-folder read benchmark — October 22, 2026

The user requested measurement while running Dragon Ball Sparking! ZERO and
rejected chat-based start/ready timing. That timing was stopped and is not used
as a loading benchmark. An automated read-only test instead measured game asset
I/O through the active folder backend and directly from its original HDD files.

## Method

`scripts/benchmark-folder-reads.ps1`: 1,024 unique pseudorandom aligned offsets,
64 KiB per read, QD1, 64 MiB per trace, seed 20261022. Offsets span three original
PAK/UCAS files. A proxy warm-up precedes three measured rounds with alternating
direct/cache order. Both test endpoints use unbuffered handles; original files
are never written. Whole-trace SHA-256 digests match in every condition.
Elapsed throughput includes copying and digest work; per-read latency measures
the seek/read call. Game/background activity remains present. Firmware caches
are not controlled and no physical-device ETW attribution was collected.
Global cache deltas include concurrent game reads and are not exclusive to the
benchmark. The filesystem, cache state and user gameplay remain running.

## Results

| Round | Direct HDD MiB/s | Warm folder cache MiB/s | HDD seconds | Cache seconds |
|---|---:|---:|---:|---:|
| 1 | 5.346 | 217.293 | 11.971 | 0.295 |
| 2 | 5.351 | 298.119 | 11.961 | 0.215 |
| 3 | 5.369 | 302.973 | 11.920 | 0.211 |

Median throughput: 5.351 versus 298.119 MiB/s, approximately 55.7 times within
this bounded random-read trace. No new cache or integrity errors were observed.
Digest: `6F6DCEDB78F3411384099DC2E5DF79A93751308538F9B3BA9A521EC6FAE11826`.

This is not the game's launch/battle-load duration, a cold full-cache benchmark,
a capacity stress test or a PrimoCache comparison. Three rounds do not establish
performance uncertainty across broader workloads. No game-loading multiplier is
inferred. The desktop's bundled October 18 results describe a different harness
and are not these measurements.

Evidence: `artifacts/benchmarks/folder-reads-20261022T140615Z/`, including full
results, individual rounds and summary. Rejected chat-marker monitoring remains
under `artifacts/benchmarks/game-live-20261022T140357Z-56605f16/`.
