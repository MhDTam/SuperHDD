# Historical local loopback backend

The desktop now starts through the [native v7 storage filter](native-transparent-filter.md).
This loopback backend is retained for dated evidence and control of an already
active owned allocation. New desktop starts do not select it. The v7 design keeps
the page file on its current drive.

This backend uses the Microsoft iSCSI initiator already included in Windows and
our user-mode block cache. It does not load the project's development kernel
driver or change Secure Boot, BCD, test signing, or certificate trust.

The requested local allocation is D: backed by a 24,999,952,384-byte payload on
the separate M: SSD (25 decimal GB, aligned to 64 KiB). Existing partitions are
not formatted, resized, or copied into a replacement filesystem.

## Control and ownership

- `SuperHDDLoopbackService` is the physical-volume controller; the separate
  `SuperHDDLoopbackLab` entry point accepts only the identified 128 MiB VHDX.
- Five pinned files reside in `Program Files\Super HDD Local Runtime`. Users
  have read/execute access; only Administrators/System may change the package.
- Prepared configuration, endpoint ownership, and the two-page checksummed
  journal reside in private `C:\ProgramData\SuperHDDLoopback` storage.
- Before handoff, fresh hardware/encryption checks and `FSCTL_LOCK_VOLUME`
  must succeed. Busy files cause refusal. Offline state is volatile.
- The payload is an exclusive, unbuffered, write-through file under the
  positively identified SSD volume. Its NTFS identity is checked on every open.
- Every acknowledged source write is flushed to the authoritative HDD. Cached
  blocks have checksums, and old cache mappings are discarded after worker loss.
- TCP reservation 33262 and early kernel/initiator peer admission prevent an
  ordinary process from impersonating the target between worker restarts.
- The status pipe authenticates its server PID against SCM and accepts only
  read-only status/telemetry requests. Setup and control use the installed UAC
  helper and a closed, bounded operation contract.

SCM controls 128/129/130 clear, pause, and resume; 132 requests a snapshot. Safe
drain uses custom control 131, which can be retried after busy-file refusal.
Standard STOP is not advertised: Windows stops delivering further controls
after accepting it, even if a service reports Running again. See Microsoft's
[service control handler documentation](https://learn.microsoft.com/en-us/windows/win32/services/service-control-handler-function).

Shutdown order is filesystem lock/dismount, projected disk offline, owned session
logout, drained worker exit and exclusive payload reacquisition, then original
source online and its verified drive letter restored. No destructor implicitly
onlines the source. A partial activation retains management if recovery still
needs attention. Paused intent is private and is applied before a new worker
starts accepting I/O.

## Qualification and current evidence

`loopback-runtime-lab-1f0d9a4853844f97a9e949a749ec73bc.json` passed the complete
earlier SCM lifecycle: real SSD reads, partial writes, worker recovery, supervisor
restart/adoption, busy drain refusal/retry, source restoration, and cleanup.
The newer interface/status/pause-persistence additions require their own current
binary-matched lab result before physical setup is enabled.

The installer defaults to the disposable role. `-EnablePhysical` requires a
successful current lab report matching all five candidate binary hashes,
including authenticated status and persisted pause recovery. The new desktop
adapter selects this backend only after that protected physical package passes
verification. The older driver backend keeps its separate release gate.

`enable-local-native-cache.ps1` is the prepared local activation/validation entry
point. It uses one CreateNew GUID-named 16 MiB D: file, verifies file identity,
unbuffered SSD hits, durable mutation against paused authoritative HDD reads,
and removes only that owned fixture. On success it leaves the requested cache
running; on failure it requests safe drain and preserves state if recovery cannot
be proved. Its client accepts no raw-device or arbitrary source path.

The service is on demand. Automatic cache startup and endpoint token
reconciliation across reboot are not qualified here. Abrupt power-loss testing
is also outside the completed evidence. The physical allocation is not active
until the physical validation report explicitly confirms it.

