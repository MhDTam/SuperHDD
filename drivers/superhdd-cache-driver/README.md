# SuperHDDCacheDriver — experimental storage code

There is now a separate **mounted-volume PnP lab candidate** in `pnp_driver.c`.
See `docs/release/2026-10-18-driver-v3.md`. Build it with `-PnpSafeBuild` or
`-PnpLabBuild` (optionally `-Analyze`). It uses its own v3 endpoint and does not
replace the shipping raw-v2 path described below. It has not been installed or
kernel-runtime-tested. Paging cache hits, persistent reuse and supported deployment
remain incomplete. The v3 ordinary-file probe refuses the physical host before
creating files; the positive signed-VM test has not run.

This is real WDM source, not an installed or qualified storage product. The default
build rejects ATTACH with STATUS_NOT_SUPPORTED. Do not change that gate for a
production machine. No installer registers a volume-class filter.

The lab implementation queues eligible 4 KiB-aligned reads to a dedicated PASSIVE_LEVEL thread,
stores payload in a separately opened SSD cache file, checks a payload checksum,
and serves valid hits from that file. Cache blocks are 64 KiB; reads may span
partial blocks. Failed speculative reads retry the requested source range.
Writes, flushes and storage-control operations invalidate before forwarding and
on completion. Hits/fills require no active mutation and an unchanged positive
epoch. Epoch exhaustion disables caching without wrapping.
Writes/flushes remain authoritative on the source. Backing failures fall back to
source reads. The index is direct mapped; the separately tested Rust algorithm
implements CLOCK. These are separate implementations, not interchangeable proofs.

The queue holds at most 16 requests of at most 1 MiB each. Paging, unsupported,
unaligned, chained/short/absent MDL operations pass through. PnP/power/shutdown
disable caching until a fresh attachment. Manual lab detach drains request rundown
and joins the reader before detaching. It requires a quiesced source; stalled I/O
and prompt cancellation remain runtime qualification blockers.

Lab attach rejects physical buses: both volumes must report BusTypeFileBackedVirtual
and one distinct disk extent. Source must be an unmounted raw direct-I/O volume;
mounted FSD devices are rejected to avoid interpreting file offsets as volume
offsets. Backing must be a newly created exclusive, noncompressed, nonencrypted,
nonsparse NTFS file beneath a pinned non-reparse parent, with a free-space reserve,
bounded EOF and capacity at most 25 decimal GB. Owned backing deletion is attempted
on failed attach and detach; a crash may still leave that file.

These are compiled guards, not kernel-runtime proof. Different virtual disk numbers
do not establish separate physical host media or that VHDs are disposable. Persistent
metadata and online resize are unsupported. Mounted NTFS/PnP integration, cancellation,
surprise removal, power and recursion/deadlock behavior still REQUIRE a lab.

To qualify: build/sign in a Windows driver lab, use only disposable VHD data
volumes in an isolated VM, enforce device identity and physical separation in the
test controller, run verifier and the matrix in docs/release/validation.md, and
collect physical source-disk I/O evidence. Never disable Secure Boot, signature
enforcement or memory integrity to load this code on the user's machine.

## Local checks and developer controller

`scripts/test-driver-state.ps1` runs the actual shared C epoch, queue and range
helpers in user mode: 87 assertions, including 32 concurrent queue contenders.
It does not load or emulate a WDM driver. Both driver builds compile with `/W4 /WX`.
Protocol v2 has a 128-byte status response; Rust checks ABI, flags, limits and version.
No privileged mutation was added to the desktop or interactive-user service pipe.

`cargo build --release -p superhdd-platform --example driver_lab` builds the lab CLI:
`status`, `attach REQUEST.json`, `pause`, `resume`, `clear`, `detach`. It requires
administrator access to a separately signed isolated-VM lab driver. Never attach on
this host. The ordinary safe build refuses lab control. The CLI installs nothing
and changes no Windows security setting.

`scripts/lab-preflight.ps1` records a read-only environment check. Follow the matrix
in `docs/release/validation.md`; compilation and helper tests do not qualify a
production accelerator.
