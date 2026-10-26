# Performance target and comparison contract

User direction, 2026-10-22: investigate a reusable signed filter and aim to
outperform PrimoCache. This is an engineering target, not an achieved result.
The request for permanent free local use remains in force; it does not authorize
a purchase or make a time-limited SDK a permanent solution.

## Architecture decision

The user subsequently accepted selected-folder caching. The current free local
candidate is the Dokan directory backend in `folder-cache-runtime.md`. Correctness
and lifecycle evidence does not establish comparative speed; the matched-workload
contract below still applies.

A filter's signature permits deployment; it does not establish its speed. A
candidate must expose read completion and write invalidation, preserve existing
D paths, and support a 25,000,000,000-byte SSD cache budget on occupied M. It must
also have a verified license permitting the intended free local use. A passive
file monitor cannot serve cached reads.

CBFS Filter's vendor documents a signed minifilter with user-process callbacks.
This is a candidate architecture, but its callback transitions and data copies
must be measured. Neither a performance advantage over PrimoCache nor a free
permanent license is established. Do not select it as the production backend on
that assumption. The existing v8 candidate remains preserved for comparison.

## Optimization hypotheses to evaluate

- Keep frequently reused game/program data resident when a large one-time scan
  would otherwise displace it. The Rust cache core already has a sequential
  bypass policy; that alone does not prove equivalent behavior in another backend.
- Investigate admission based on reuse, bounded read-ahead, coalescing adjacent
  SSD reads, and indexing that avoids scanning cache entries for every read.
- Measure unnecessary callback traffic, copying, contention, metadata lookup,
  and integrity verification before changing them. Preserve data verification
  and write coherence; removing checks is not an acceptable speed improvement.
- Do not intercept Windows RAM-cache hits merely to report extra cache activity.
  No dedicated RAM data tier is requested, but indexes, I/O buffers and Windows'
  own cache still consume memory and must be distinguished in measurements.
- Preserve write-through behavior. A write acknowledged before reaching D is a
  different durability contract and cannot be used for this comparison.

These are hypotheses, not implemented features or claims about PrimoCache's
undocumented internals. Its public specification already lists prefetching and
an adaptive replacement algorithm; do not claim those ideas alone are unique.

## Fair comparison

Compare direct D access, candidate bypass, candidate active, and PrimoCache active
with only one accelerator running at a time. Record versions, configuration,
effective cache-data capacity, backing allocation, metadata memory and CPU cost.
Use the same hardware, offsets, data, workload and warm-up schedule. For the
matched SSD-only comparison, set PrimoCache L1 to zero and disable deferred
writes. Report any configuration differences explicitly.

The primary metric is elapsed completion time for representative workloads the
user actually runs. Initially include repeated random reads (4 KiB and 64 KiB,
queue depths 1 and 8), application/game loading, mixed reads and writes, sequential
scans, and a hot working set followed by a scan larger than the cache. Include
working sets below and above capacity, cache misses, and warm cache hits.

Run at least ten paired application-load repetitions per condition in alternating
order; report sample counts, median, spread and paired uncertainty. For I/O
latency percentiles, collect a sufficiently large operation sample, not merely
ten timings. Record source/SSD physical I/O so Windows RAM-cache hits are not
misreported as SSD-cache hits. Restarting an app alone does not create cold OS
cache. Use aligned unbuffered I/O only for controlled owned-file microbenchmarks;
keep real application's normal buffered I/O for application-load results.

Provisional acceptance target: at least 10% lower median time than matched
PrimoCache in the chosen primary application workload, with uncertainty excluding
zero improvement, and no material unexplained regression in tails, miss handling,
CPU use or memory use. Publish per-workload results; never generalize a win in one
workload into "faster in every case." Correct bytes, concurrent invalidation,
mapped-write coherence and safe process failure are prerequisites.

## Execution boundaries and present evidence

All early integration tests use new disposable source/cache files. A vendor L2
setup requiring a dedicated partition must be tested in an isolated environment;
do not format or repartition occupied M for the comparison. Do not install a
trial, attach a new host filter, reboot, or alter boot security just to satisfy
this document. The current D pagefile remains unchanged.

Earlier direct API benchmarks show a benefit versus uncached HDD reads within
that harness. They are neither transparent host acceleration nor a head-to-head
PrimoCache comparison. The later v8 VM proof likewise does not establish host
performance or superiority to PrimoCache. No such comparative result exists yet.

## Primary references

- [CBFS Filter architecture and licensing](https://www.callback.com/cbfsfilter)
- [PrimoCache features](https://www.romexsoftware.com/en-us/primo-cache/specification.html)
- [PrimoCache configuration: disabling L1 and L2 behavior](https://www.romexsoftware.com/en-us/primo-cache/terms-configuration.html)
- [Existing controlled API results](benchmarks/2026-10-18-audit.md)
