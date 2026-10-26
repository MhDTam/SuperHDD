# Native storage filter v7

The current desktop development path is a WDM upper filter on the selected
storage-volume device. It serves eligible ordinary reads from an owned SSD cache
while the source remains mounted on its original Windows storage path. It does
not use the loopback/iSCSI handoff and does not move the Windows page file.

The first coexistence implementation deliberately forwards **all paging reads**
to the source. This includes memory-mapped reads when they reach storage as paging
I/O. Those requests never wait for the cache worker or the SSD backing path.
Paging writes, like ordinary writes, invalidate overlapping cache entries before
being forwarded. No deferred source writes or dedicated RAM data cache are used.
Nonpaged index and I/O metadata still require memory.

## Implementation

- `drivers/superhdd-cache-driver/v7/` has its own endpoint, version-7 ABI and
  independently tracked paging/dump/hibernation notifications. Failed lower-stack
  notifications roll back their counters. Paging counts do not disable ordinary
  caching; dump/hibernation usage still blocks it. Query-stop/remove requests are
  refused while special files are present. Power transitions invalidate trust.
- `transparent_driver.rs` validates the new ABI. `transparent_cache.rs` uses the
  existing bounded, protected VHDX/locked-RAW backing on the separate SSD. It does
  not format or repartition the physical SSD or change the source mount.
- `transparent_host.rs` owns the LocalSystem session and durable recovery state.
  The public SCM-authenticated pipe is read-only; privileged controls use the
  existing protected admin pipe. The v5 service entry point remains separate.
- `transparent_runtime.rs` and `transparent_ops.rs` connect the desktop and fixed
  installed helper to v7. Both require exact qualified package hashes. Native v6
  allocations can still be controlled if one is already active; new desktop
  starts use v7. The dormant v6 package does not select that backend.
- `enroll-transparent-source.ps1` touches only the exact selected volume's
  per-device filter property. On an active paging source it queues enrollment for
  an ordinary reboot, preserving the page file and avoiding a live device restart.
  It never modifies global disk-class filters or host boot-security settings.

## Verified scope

`artifacts/transparent-session.json` records the direct kernel probe. A real
128 MiB page file remained active on the guest source. The warm 96-request round
recorded 192 cache hits, 12,582,912 SSD-read bytes and zero additional source-read
bytes. Byte oracles covered ordinary and mapped reads, partial writes, pause,
clear, stop and the original source after detach. The successful mapped-read
round also observed 21 new paging bypasses. Initial failed probe attempts are
retained in the run directory; the final successful retry and its client hash are
recorded separately from those failures.

`artifacts/runtime/kernel-lab/transparent-service.json` records the complete SCM
cycle with another real source page file: byte-correct reads/writes, same-container
reopen, actual SCM automatic restart after killing the exact owner process,
persistent-record revalidation, protected controls and restricted-user denial.
The wrapper restores guest paging policy, removes its per-device filter and
temporary disks, checks cleanup after a cold boot and shuts down the VM.

These tests use locally test-signed **VM-only** SAS builds in the provisioned,
network-isolated Server guest. They are not physical Windows 10/11 qualification
or evidence of acceleration on the user's D drive. The physical build compiles
and passes static analysis but is unsigned. A Windows-accepted signature and
physical-source, low-memory, power, PnP and lifecycle qualification remain required.
The current source cannot be described as a feature-equivalent PrimoCache release:
paging reads are deliberately passed through, not accelerated.

The [design review](native-filter-design-review.md) distinguishes these direct
I/O correctness checks from application-speed evidence and records the next
paging-path milestone and the host-specific signing alternatives reviewed.

## Build and inspect

```powershell
.\scripts\test-transparent-policy.ps1
.\scripts\build-transparent-driver.ps1 -Analyze
.\scripts\build-transparent-driver.ps1 -VmTest -Analyze
```

The desktop embeds `SUPERHDD_QUALIFIED_FILTER7_SHA256` and
`SUPERHDD_QUALIFIED_FILTER7_SERVICE_SHA256` only for a qualified package. Development
builds leave these absent and reject activation before UAC or any storage change.
`stage-transparent-runtime.ps1` creates only a workspace review package and
reports signature status and exact tested/staged hashes; it never installs it.

The latest desktop and service also persist the selected source, SSD and budget
in the public runtime state, so the desktop displays the actual saved allocation.
The 21 desktop UI checks passed. The final SCM retest completed successfully in
run `42017c57ac174fd9aec1644a1890e6dd` on 2026-10-21. All 13 service checks and the
cold-boot cleanup check passed. Its service SHA-256 matches the staged service:
`8FAC384E026525179D2E16D4C1F2ED2A5E874B953219DF2BC359DE0502527309`.
The VM ended off with zero assigned RAM and no active host cache driver.
The earlier `transparent-runtime-preview` preserves the pre-retest snapshot;
`transparent-runtime-candidate` contains the exact service with its completed
SCM proof. Both remain unsigned, unqualified workspace artifacts, not installed
host acceleration. Ordinary staging requires a complete SCM proof for the exact
service binary.
