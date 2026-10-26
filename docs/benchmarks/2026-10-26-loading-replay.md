# Loading replay: previous vs revised cache engine (2026-10-26)

`scripts/run-loading-replay.ps1` compares the cache engine before and after the
2026-10-26 changes on new disposable files only. No game folder, game profile
or existing pool was touched. Evidence: `artifacts/loading-replay/<run>/result.json`.

## Method

- Fixture: 16 files × 192 MiB (3 GiB) on D: (Hitachi HUA722020ALA331 HDD).
  Pools on M: (Samsung 980 PRO), in a new per-run directory.
- Replay (`tools/loading-cache/loading-replay.cpp`): an Unreal-style async pak
  load. Four threads, each with its own handles, issue 64 KiB-aligned reads:
  2,400 scattered 16/64 KiB reads, 24 sequential runs of 1–4 MiB and four
  16 MiB runs, in a seeded shuffled order (254 MiB, about 3,640 reads). The
  same seed is used for every pass, i.e. loading the same level again. Every
  byte is verified.
- Both workers are built from their own sources (`artifacts/loading-replay/baseline-src`
  is the previous engine) with `SUPERHDD_STORAGE_BENCHMARK`: the replay client,
  source reads and SSD reads are unbuffered, so Windows' RAM cache cannot serve
  repeats. This models a cold OS cache, as after a reboot. Normal game workers
  use buffered I/O, where RAM can additionally help repeat loads.
- Passes: direct HDD ×2; mounted cold; 2nd and 3rd identical loads (waiting for
  background fills to finish between passes); stop and restart the worker on
  the same pool (a game relaunch or reboot); one more load.

## Results

Run `af216bae1c674239a83bd6190422f055`, 2,048 MiB pool (larger than the replay):

| Pass | Previous engine | Revised engine |
|---|---|---|
| Direct HDD, no cache | 30.1 s (p95 79 ms) | same |
| 1st load, cold cache | 30.5 s, 0% SSD | 29.3 s, 2.8% SSD |
| 2nd load | **36.3 s**, 0.9% SSD, 3,701 lock bypasses | **0.47 s**, 100% SSD (p95 0.46 ms) |
| 3rd load | 1.34 s, 87.1% SSD | 0.50 s, 100% SSD |
| After worker restart | 30.1 s, 0% SSD (cache wiped) | **0.50 s**, 100% SSD |

The previous engine's second load was about 20% slower than the HDD alone: its
second-touch fills competed for the pool mutex, and reads that found it busy
went to the HDD. It needed three loads to help, and lost everything on restart.

Run `00804c3925e34a2e84dad8a694907b56`, 128 MiB pool (half the replay's working
set, uniformly random): neither engine helps (previous 30.2–31.3 s, revised
29.4–30.3 s, HDD 31.0–32.0 s). LRU cannot hold a uniformly random working set
twice its size. The revised engine had no lock bypasses (previous: about 3,800
per pass). Size a configuration's pool to cover the game data you want cached.

## Limits

These are storage replays, not game measurements. They do not include the
game's own CPU, decompression, shader or network time, which also bound real
loading screens, nor Dokan behaviour with buffered game I/O. A real comparison
needs repeated identical loads in the actual game (see
`game-comparison-protocol.md`). A 2nd load at 0.5 s here means storage stops
being the bottleneck for that data, not that the loading screen takes 0.5 s.
