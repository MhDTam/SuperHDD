# Performance time windows

The Performance & diagnostics period buttons select real recorded intervals for both charts and activity counters. Live shows the last two minutes; the other windows are one hour, one day, seven days and thirty days. Cache occupancy is explicitly a current value, not a period total.

`apps/desktop/src/performance-history.ts` stores timestamped counter differences, rather than plotting cumulative session counters repeatedly. Hit rate is the ratio of summed hits to summed hits and misses; throughput is transferred bytes divided by actual recorded seconds. Idle samples have zero throughput and no hit-rate observation. Session generation changes, any counter rollback, paused/bypassed engines, clock rollback and collection gaps longer than ten seconds establish a new baseline without generating a delta.

Collection runs across all application pages while the window is visible. It does not collect while the UI is closed or hidden and does not reconstruct missing activity. History is independent of the folder worker and never starts, stops or clears the user's cache. Previously recorded periods can still be inspected after the engine stops.

History is saved to the webview's local storage under `superhdd-performance-history-v1`, at most every fifteen seconds and on page hide or orderly UI cleanup. A forced termination can lose the most recent fifteen seconds. Save failures leave the in-memory history available and produce an explicit message. No file names or game paths are included.

Storage uses bounded tiers: individual samples for two minutes, fifteen-second aggregates for an hour, minute aggregates for a day, and half-hour aggregates for thirty days. Each tier also has a hard limit of 6,000 entries. A period excludes a coarse bucket that overlaps its lower boundary instead of assuming a distribution of reads inside that bucket. Charts further aggregate adjacent observations to about one hundred points, preserve collection gaps and use a numeric axis spanning the full selected window. Sparse observations remain visible as dots. Unrecorded time stays blank.

`tests/ui/performance-history.spec.ts` checks all five windows against distinct recorded intervals, counter reset handling, retention and invalid storage. Its browser regression checks changed axes and totals, recording across page navigation, reload persistence, inspection after stopping and Arabic layout. The complete 40-test UI suite passed for this change.
