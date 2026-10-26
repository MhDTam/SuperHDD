# Release qualification — native lab proof, production qualification pending

The current [direct-v5 candidate](2026-10-19-direct-v5.md) adds a production
source policy and gated desktop/runtime ownership. Its exact-image isolated VM
run passes the service/data/crash-recovery cycle and cleanup verification after
reboot. Local results are in `artifacts/direct-v5-evidence.json`. Physical source,
active-cache cold boot/power loss, v5 stress, signed runtime lifecycle and Windows
10/11 hardware qualification remain pending. The results below describe the
earlier v4 and desktop snapshots; they are not automatically transferred to v5.

The v4 update is recorded in `2026-10-19-driver-v4.md`. Its RAW-volume backend,
paging reads and source-revalidated persistent index have run through a locally
test-signed kernel driver in the isolated Server 2022 guest. Ordinary-file data,
mapped paging reads, source writes, reopen/revalidation, corruption fallback,
pause/clear/disable are recorded with binary hashes in
`artifacts/raw-source-evidence.json`. Production Windows 10/11 hardware,
cancellation/power fault matrices and service deployment remain unqualified.

`raw_benchmark` additionally places a disposable source VHD on the real D: HDD
and the RAW cache on M: NVMe, keeping the driver inside the guest. Fixed QD1 4 KiB
reads are byte-checked across baseline/cold/warm phases. Whole-physical-disk
counter deltas are recorded independently of kernel counters. They are scoped
to that measurement window and do not replace a per-file ETW trace, game test,
physical-host filter deployment or full Windows 10/11 qualification. The first
run failed to show warm acceleration and is retained as failure evidence.
The final 512 MiB trace passed: 11.53x warm speedup, matching data hashes,
zero driver errors, 8,192 SSD hits and zero whole-D: physical reads during
the warm window. Routine volume-identity/cluster queries now retain the hot
set and are covered by a signed-VM regression probe. Broader reliability and
per-file physical tracing rows below remain pending.

Bounded v4 stress now covers 32-request overlapped batches, actual queue
cancellation, clear/pause/disable while driver reads are pending, and 32 independent
source writes followed by 64 full-block freshness checks. Six phases passed with
standard Driver Verifier and Force Pending I/O on the guest-only test image.
The reports bind the signed image, tool and test-source hashes. This does not
establish active-child cancellation (its counter remained zero), deliberate
allocation failures (none observed), power/removal or sustained-load qualification.
The download pause/resume integration fixture now gates its first partial response
until pause is requested, avoiding a scheduler-dependent full-download race.

The 2026-10-19 desktop connects read-only v4 telemetry and removes vendor
discovery/inspection code. Its rebuilt binary passed 18 native checks, ordinary
startup/single instance and installer preservation; 12 browser cases include
mocked Arabic v4 diagnostics. `scripts/test-native-bridge.ps1` adds a distinct
SCM-service-to-v4 positive VM contract test. Its initial elevation cancellation
was resolved. The first actual run found transient pipe contention, now fixed
with a bounded client retry and a 32-request real-pipe regression check. The
current deployed service and probe passed the positive VM contract, including
ordinary reads, actual SSD-hit deltas and detached/null identities. The service
was removed, the cache detached and the VM shut down. This is distinct from old
driver benchmarks and does not qualify physical-source activation.

Never use irreplaceable user data. Build a separately signed lab package, use an
isolated Windows VM and fresh disposable data VHDs, and retain baseline images.
Do not disable platform security protections on the user's machine.

## Mandatory transparent cache proof

1. Inventory source/cache physical identities; reject same-device, source system,
   encrypted, removable, unsupported filesystem and recursive backing layouts.
2. Create a disposable NTFS HDD test volume and separate SSD backing store.
3. Load the appropriately signed lab filter in the isolated environment.
4. Use noncached aligned reads (e.g. DiskSpd with the appropriate cache flags).
5. Record byte-correct cold miss and SSD population.
6. Repeat identical reads; compare the driver's SSD-hit counters with storage-stack
   tracing and prove corresponding physical HDD reads were avoided.
7. Test concurrent/overlapping/partial writes; verify no stale bytes after writes
   complete. Include cached, noncached, paging and memory-mapped workloads.
8. Detach and compare the original source byte-for-byte.

The Rust `proof` example proves algorithm/source-API behavior only. It must never
be cited as completion of steps 3–8 or evidence of physical HDD performance.

The v2 driver lab accepts **unmounted raw VHD sources only**. The separate v3
`pnp_driver.c` implements a mounted NTFS/PnP candidate, but has not been deployed,
loaded or runtime-qualified. Paging cache hits and persistent reuse remain
unsupported. Per-device filter enrollment and the ordinary-file `pnp_probe`
must run in a separately signed isolated VM before advancing this matrix. Host shared-C
helper tests and static analysis are additional source checks, not kernel-runtime
proof. Never remove the physical-bus gate merely to run a user-disk benchmark.

The v3 probe uses ordinary `CreateFile`/read/write paths and no `FileCache` API;
its internal-driver counters do not prove physical source-disk I/O avoidance.
It is compiled, but only its refusal to run on the physical host has been tested.
Do not turn the desktop activation gate on after a compilation or probe refusal.

## Reliability matrix

- Cancellation at queue, source completion, cache population and detach boundaries.
- Partial reads, short source reads, MDL-less I/O, alignment and overflow.
- FIFO contention, queue saturation and bounded kernel-memory use.
- Cache collision/full conditions; independent source/backing error injection.
- SSD removal, HDD removal, surprise PnP removal, sleep/hibernate/resume and power loss.
- Flush/FUA semantics, TRIM/DSM, volume lifecycle and storage IOCTL pass-through.
- Service termination/restart, GUI termination, attach/detach under load.
- Corrupt metadata/payload, offline source writes and safe cache invalidation.
- Driver Verifier, static analysis, HLK where applicable, supported Windows 10/11
  builds, secure boot and memory-integrity compatibility.
- Driver signing and installer/uninstaller rollback with active cache tasks.

## Download and UI checks completed on 2026-10-18

Changing/weak ETags, ignored ranges, 416 recovery, redirects, truncated bodies,
single-owner staging, four-worker resume limits, malformed recovery manifests,
interrupted destination copies, hash mismatches, no-overwrite recovery and removal
that preserves published files are covered by passing integration tests. A real
worker was killed during an E: destination copy and recovered with matching SHA-256.
The concurrent quota reservation uses a shared lock; general disk-full/removal
fault injection and a long-duration concurrent quota stress test remain pending.

Local pipe tests cover stalled, malformed and oversized clients. Service installation,
install-path tampering and the deployed SCM authentication matrix remain pending.
Browser tests cover keyboard focus/Escape/restore, 2x-DPI layout, resize and Arabic;
native WebView2 checks are recorded in the final audit. This does not establish a
complete accessibility/contrast audit or coverage of all supported Windows builds.

## Performance evidence

Record baseline, cold and warm tests, random/sequential workloads, latency,
driver CPU, kernel pool, actual source/SSD I/O and conditions where caching loses.
No multiplier or benchmark claim may be published until collected and repeatable.
