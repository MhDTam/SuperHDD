# Game resource trace: measured observations and remaining limit

Trace: `artifacts/benchmarks/game-trace-20261022T144910Z-559a61d6/`.
Recorder requested 45 seconds; its sampler spans 44.905 seconds. The ETW file
spans 52.448 seconds, including saving/rundown. These intervals are distinct.
ETW reports zero lost events and zero lost buffers. CPU, disk/file I/O and hard
fault exports are retained alongside `analysis.json` and raw samples.

## Observations

- During the sampler interval, folder source-read bytes increased by zero,
  cache-read bytes by 2,712,450, hits by 42 and misses by zero. No new cache or
  integrity errors were recorded. This is a very light read interval, without
  independently verified battle loading endpoints.
- Over that interval, worker CPU increased by 4.516 CPU-seconds: approximately
  0.63% of all 16 logical processors, or 10.06% of one logical processor. Working
  set was 46.02 MiB. These are process totals, not incremental overhead measured
  against a matched no-cache run. Whole-trace sampled CPU gives about 0.62%.
- In the full ETW interval, `cache.bin` on M received 131 physical reads totaling
  8,585,216 bytes (8.19 MiB). This independently establishes SSD cache-file I/O,
  rather than treating API counters as physical-device reads. It does not match
  the shorter sampler byte count because the recording intervals differ.
- The named-file D reads in the disk summary belong to `D:\pagefile.sys`,
  totaling 12,894,208 bytes (12.30 MiB). No other named D file reads appear in
  that summary. Pagefile traffic must not be counted as game-asset cache misses.
  The pagefile location/settings were not changed.
- Read-only driver discovery shows the existing Fortect `cbfilter24-0` filter
  and Dokan loaded. The former is not our cache backend and its presence is not
  evidence that a second accelerator is caching. Preserve this environmental
  detail for matched comparisons; no driver was disabled or changed.

## What cannot be concluded

This interval does not establish game loading time, a loading speedup, a loading
bottleneck or superiority to PrimoCache. Low storage activity outside a known
loading region cannot explain an unobserved loading delay. CPU/shader/GPU work
must be correlated to verified scene transitions before such a conclusion.

Computer Use window discovery works, but image capture fails for both the game
and Super HDD. Capture still failed after the user changed to windowed mode,
after activation, and after one JavaScript-session reset. Exact errors:
`FrameArrived timed out: timed out waiting on channel` and
`window capture timed out: timed out waiting on channel`.
The user explicitly reauthorized Computer Use after its earlier Escape stop;
the current limitation is failed image capture. No blind game input was sent.

Consequently the four-condition, ten-pair game comparison remains incomplete.
It requires restored reliable visual capture or a trustworthy game event log.
The reviewed game Logs directory contained no usable loading log.

## Candidate engineering investigation

The worker consumed CPU despite few cache reads. Its current status routine
scans the allocated slot array and cached-file indexes every 200 ms. This is a
candidate for a future stack-profile investigation, not a proven attribution
of all CPU time to that routine. No cache-engine code was changed during this
analysis and the running session remains qualified and active.
