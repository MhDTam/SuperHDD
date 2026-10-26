# Storage-cache design review — 2026-10-21

## Decision

Keep the target interaction: select D as source, M as SSD cache, and a 25 GB
budget. The native filter remains the appropriate direction for transparent
acceleration of the existing mounted source. The current v7 candidate proves
bounded data correctness and service recovery, not application speedup or
physical-machine readiness. Do not change boot trust or host partitions as part
of this review.

## A functional gap precedes signing

`v7/volume_driver.c` forwards every read marked `IRP_PAGING_IO` or
`IRP_SYNCHRONOUS_PAGING_IO`. These flags do not identify only pagefile.sys:
memory-mapped file reads can also reach storage as paging I/O.

The cache-hit rounds in `transparent_session_probe.rs` use `0x20000000`
(`FILE_FLAG_NO_BUFFERING`). The separate mapped-file check proves byte correctness
and observed paging bypasses, not mapped-file acceleration. The earlier successful
kernel report records 21 mapped-read paging bypasses. Therefore the present proof
does not establish a benefit for normal buffered or mapped application workloads.

Do not fix this by deleting the paging bypass. The current reader allocates
scratch memory and child requests while servicing reads, and the locked RAW cache
volume is still inside a VHDX hosted by the SSD's file system. A RAW inner volume
does not establish that the entire underlying path is paging-safe. All code,
buffers, metadata, queues and backing dependencies need a paging-path audit.

The next engineering milestone is a bounded paging-capable read path with
reserved nonpaged resources and a backing path whose dependencies are proven.
If resources or a trusted cached block are unavailable, preserve source semantics
and forward the original request. Concurrent writes must invalidate overlapping
entries before a cached read can expose stale data. This is a design requirement,
not an implemented or qualified feature of v7.

## Required performance evidence

- Exercise normal buffered reads and memory-mapped reads, in addition to direct
  unbuffered reads. Verify bytes on every path.
- Measure elapsed time, source bytes, SSD bytes, hits and paging bypasses for the
  same workload with the filter paused and active.
- Distinguish Windows RAM-cache hits from SSD-cache hits. A resident read with no
  observed storage I/O provides no evidence of SSD acceleration.
- Exercise low-memory operation, concurrent writes, backing I/O errors and
  shutdown/power transitions in the disposable guest before physical activation.
- Measure reopen/reboot behavior separately: persisted records currently require
  source revalidation. Their presence alone does not prove a faster first read
  after a restart.

## Loading the driver on this machine

The current physical binary is unsigned; kernel-policy verification rejected it.
The host is Windows 10 Pro build 19045. A conventional trusted signing path
through Microsoft's hardware dashboard remains applicable; attestation signing
does not itself establish compatibility or performance, and need not mean public
Windows Update distribution.

Microsoft's newer Custom Kernel Signers route was also reviewed. Its detailed
requirements specify Windows 11 24H2 or later with the relevant 2026 update,
initial system setup/reset, and a policy rooted in controlled Secure Boot keys.
It is not an in-place solution for this Windows 10 installation and conflicts
with preserving the current boot configuration. No trust-policy changes were
made. Reusing an unrelated signed driver does not sign or authorize our binary.

## Sources

- [Storage paging-path restrictions](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/restrictions-on-pageable-code-in-storage-drivers)
- [Microsoft driver signing options](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings)
- [Custom Kernel Signers and supported platforms](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/design/custom-kernel-signers)
- Local evidence: `artifacts/transparent-session.json`,
  `artifacts/runtime/kernel-lab/transparent-service.json`,
  `artifacts/transparent-driver-signature.json`, and
  `artifacts/diagnostics/cache-partition-feasibility.json`.
