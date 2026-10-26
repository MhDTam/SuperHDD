# Local block-cache integration probe

This optional probe connects the existing Rust `FileCache` to an iSCSI target
through a small synchronous native ABI. It is **not** the desktop activation
backend. Its source is either a new 128 MiB D: image or the raw device of this
run's new 128 MiB VHDX. The latter requires the exact Microsoft VHDX dependency,
host volume, filename, creation identity, size and offline state. The real D:
device cannot pass that guard. Its 32 MiB M: cache lives in the workspace runtime
folder. D: remains mounted normally.

The image uses exclusive, unbuffered I/O. The cache payload lives in the SSD
file; buffers in RAM are transient I/O buffers. The target can expose either
512-byte or 4096-byte sectors and requests are bounded to 1 MiB. A 512-byte
write uses serialized 4 KiB read/modify/write when needed, preserving adjacent
source sectors under the exclusive source lease. Every accepted write invalidates overlapping
cache entries and flushes the authoritative source before completion. The
target library completes SYNCHRONIZE CACHE locally, so this every-write durability
rule is required. Cache mappings are discarded on restart.

## Reproduce

From the project root in PowerShell:

```powershell
.\scripts\build-iscsi-probe.ps1
.\scripts\test-iscsi-protocol.ps1
.\scripts\test-iscsi-process-recovery.ps1
```

The second command needs no administrator access, Windows disk registration,
MSiSCSI service change, driver installation or boot change. It binds only
127.0.0.1:33261. The process recovery test likewise needs no Windows elevation;
it uses a target child on 127.0.0.1:33260 and a separate user-mode client.
The separate Windows initiator test uses 127.0.0.1:33260 and
requires ordinary Windows elevation:

```powershell
.\scripts\run-iscsi-cache.ps1
# Extended interruption/reopen test, matching the existing HDD's sector size:
.\scripts\run-iscsi-cache.ps1 -Recovery -SectorBytes 512
# Existing NTFS files inside an owned VHDX, native SDK handoff and recovery:
.\scripts\run-iscsi-cache.ps1 -Handoff -NativeInitiator -SectorBytes 512
.\scripts\run-iscsi-cache.ps1 -Handoff -Recovery -NativeInitiator -SectorBytes 512
```

That test only initializes/formats its new 128 MiB iSCSI disk after matching
its session, run-specific serial, size, bus and non-system identity. It uses
private S: and restores the Microsoft initiator's previous state after teardown.
It never enrolls, dismounts or changes the letters of existing D: or M: volumes.

## Existing-file handoff, verified 2026-10-21

Run `f0a17278e8954689a957c059e1ae74b2` passed the native handoff and cleanup.
An ordinary GPT/NTFS source already contained the complete 16 MiB fixture and
another preserved file before being handed to the cache. No formatting occurred
on the projected iSCSI disk. The source and projected volume both refused a lock
while a fixture file was open, then locked/dismounted successfully after closure.
Disk offline changes used `IOCTL_DISK_SET_DISK_ATTRIBUTES` with `Persist=false`.
The SDK initiator used a temporary static target, no discovery portal and no
persistent login. The 4 KiB acknowledged mutation survived returning to the
original source. Owned files were removed, MSiSCSI returned to Stopped/Disabled,
and Secure Boot, code integrity and the real D:/M: identities were unchanged.

That run also inspected the real D: read-only: a separate SATA HDD, GPT data
partition plus MSR, NTFS, 512-byte logical sectors and FullyDecrypted/0% BitLocker.
It did not lock, offline, export or activate caching on that physical disk.

The private transport is now built from pinned upstream source with two narrow
adapters: complete short socket sends (including zero-progress rejection and a
send timeout), and a connection admission callback before any PDU/session handling.
The native test admits only the fresh owner PID of MSiSCSI reported by SCM, with
its process pinned and status rechecked, or kernel PID 4. It matches the full
established loopback TCP tuple using `GetExtendedTcpTable`. Initiator-supplied
names and session IDs never authorize a connection. The native test verified
that a user-process initiator was refused and MSiSCSI accepted. Build manifests
record the actual adapter, source and binary hashes.

Run `2b14fba3b0114b87af1513b811416ea2` reused the same SSD allocation with zero
initial mappings after forced worker termination, but its automated handoff test
failed when Windows returned the projected disk offline and S: disappeared.
Its separate guarded recovery passed, including all 256 blocks/16 MiB of the
mutated payload through the cache and then the original source, and full cleanup.
Evidence: `artifacts/iscsi-handoff-recovery-2b14fba3b0114b87af1513b811416ea2.json`.
The automated path now resolves the source number before interruption and
explicitly revalidates, onlines and remounts the owned projected GPT partition.
That failed run is retained as failed; its recovery does not relabel it.

Run `bf4474d9f34b409fa3c954b30094d791` then passed the complete automated
handoff plus forced worker interruption/reconnect. The original SSD file retained
its creation identity and 32 MiB length; the recovered provider began with zero
occupancy/mappings. Windows' projected GPT disk was revalidated and remounted,
all 256 blocks/16 MiB of the modified payload passed the byte oracle, and the
warm pass read only SSD for the fixture. Returning to the original source also
preserved the modification and the other file. All owned files/mounts were removed,
the initiator returned to Stopped/Disabled and host boot/D:/M: identities matched.
Evidence: `artifacts/iscsi-handoff-bf4474d9f34b409fa3c954b30094d791.json`.

Run `b931f75919634a8785694d1108b1e863` passed the same complete lifecycle using
the production-oriented `data_source::open_offline_device` primitive and a
System/Administrators-only SSD directory. The payload was opened exclusively with
unbuffered/write-through I/O. A private owner record binds its NTFS volume serial,
file index and creation time; those identities were verified before reuse. Parent
directories stayed pinned for the provider's lifetime. The recovered provider
reported both `ssdPayloadProtected` and `ssdFileExclusive`, and all 16 MiB of the
modified payload passed after reconnect. Source restoration, cleanup and boot/
parent identity checks all passed.

The Rust service now supports `--loopback-preflight` with the existing bounded
encoded start-request contract. Its new native context validates the real D:/M:
identities, hardware media descriptors, non-system roles, NTFS health, verified
source decryption, GPT layout, sector size, free-space reserve and the exact
24,999,952,384-byte request. `artifacts/loopback-preflight.json` and
`artifacts/loopback-integration.json` passed on this PC. This is read-only service
integration: no permanent service/worker installation or real-D activation is
performed by that command. The new service build is isolated under
`target/loopback-service`; existing release and kernel-lab binaries are unchanged.

## Earlier file-image recovery, verified 2026-10-20

The latest native Windows run `7eb6fed2054c479ca3c1c9db6d47bfe8` passed,
including forced process interruption, Microsoft initiator reconnect, NTFS
extent-based measurement and cleanup. It used a new 128 MiB source image,
32 MiB SSD caches, a 16 MiB fixture, and 512-byte sectors matching D:.
After reopening the source with empty cache mappings, both the original data
and the acknowledged 4 KiB mutation passed the unbuffered byte oracle. A
transient ERROR_NO_SUCH_DEVICE during the first recovered read was retried;
the second read attempt passed. Each warm fixture pass added 192 block hits
and 12 MiB SSD reads with zero fixture-attributed source reads. The initial
global warm counter also included 64 KiB of unrelated NTFS source reads;
those remain visible separately rather than being discarded from the report.
Owned source/cache files and the private mount were removed, MSiSCSI returned
to Stopped/Disabled, and boot state and D:/M: parent identities were unchanged.
Evidence: `artifacts/iscsi-local-7eb6fed2054c479ca3c1c9db6d47bfe8.json`.

`artifacts/iscsi-protocol-latest.json`, run
`04edf6d5314943fd98dd272bb124bc71`, records:

- 96 repeated reads, 6 MiB byte-oracle verification per pass;
- 96 additional SSD hits, 6 MiB SSD read bytes, zero additional source read bytes;
- write-through updates, partial-block invalidation, pause/resume and bounds errors;
- source handles excluded while active;
- owned files removed, listener closed, boot state and MSiSCSI state unchanged.

The user-mode library client throws `InvalidDataException` on a read sense
response. The negative read test accepts that behavior, then checks a valid read
on the same connection. The write error must return failure.

The Windows initiator/NTFS test also passed its byte-oracle checks in run
`9f8d72e4e94f4dc7aefcd8f44714fcf5`: Windows registered the new 128 MiB,
4096-byte-sector iSCSI disk, initialized GPT and NTFS, wrote a 16 MiB fixture,
verified 96 unbuffered cold and warm reads, bypassed the cache when paused, and
verified a partial write after resuming. Warm reads added zero source bytes.
The first run's teardown failed because Windows 10's installed cmdlet CDXML
declares an Int32 port for Remove while its CIM provider requires UInt16.
`scripts/remove-private-iscsi-portal.ps1` uses the local provider's exact type
and preserves unspecified initiator parameters as unspecified. A separately
recorded recovery removed the owned files/portal and restored the initiator:
`artifacts/runtime/iscsi-local-9f8d72e4e94f4dc7aefcd8f44714fcf5/cleanup-recovery.json`.

The complete rerun `1f0064f9d9884a3e9138bdb3a5b11adf` passed with the
cleanup fix, as recorded by `artifacts/iscsi-local-latest.json` and its archived
run record. All read/write, pause/resume and partial-write checks passed. The
warm pass added 192 internal cache hits and 12 MiB of SSD reads with zero HDD
source bytes (the fixture oracle verifies 6 MiB of payload). The internal I/O
counters include filesystem requests and cache-block reads, so they are not a
second measure of fixture payload or an application-wide performance claim.
Both disposable files and the private mount were removed, MSiSCSI returned to
Stopped/Disabled, and boot state and D:/M: parent identities were unchanged.

## Recovery work, 2026-10-20

`cache_probe_reopen` accepts only a recorded run ID. It requires the disposable
source to retain its recorded creation time and size, rejects a reparse point,
and creates a different new SSD cache file. Source ownership records in this
private probe are test guards, not a production privilege boundary. Cache
mappings are always rebuilt; no stale cache mapping is restored from disk.

Protocol run `f7220f988cbb4e1a9bdde4c3e38e2cc9` passed after adding a 512-byte
write spanning a 4 KiB boundary and verifying every neighboring byte. Its 259
write operations include two RMW setup writes, 256 fixture writes and one
partial update. Warm reads added 96 hits and 6 MiB of SSD reads with zero
additional source bytes. Files were removed and service/boot state was unchanged.

Native 512-byte-sector run `f0b8d10ec35b47fe8e024202ba8d830e` passed initial
GPT/NTFS, byte-oracle reads, pause/resume and partial-write checks, then failed
the first read immediately after forcibly stopping/reopening the target process:
Windows returned ERROR_NO_SUCH_DEVICE (433) while the device was returning.
Cleanup passed. An earlier 4096-byte run `224c6bde32bd430d8e1fe82c65472610`
hit the same arrival condition. These runs do not establish reconnect success.
The extended test now retries only explicit native device-arrival errors for
at most 60 seconds; it never retries a byte-oracle mismatch. Completion of that
revised native test subsequently passed in run
`7eb6fed2054c479ca3c1c9db6d47bfe8`, recorded above. The normal file-only test
also remains available without interruption testing.

Native run `67512e7a51904386b6d8ddca8576996a` stopped before interruption:
all 96 warm fixture reads passed and produced 192 hits, but the whole-disk
counter also included 128 KiB of background NTFS reads. Cleanup passed. The
test now registers the fixture's actual extents, obtained with Microsoft's
[FSCTL_GET_RETRIEVAL_POINTERS](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_get_retrieval_pointers),
and separately reports cache traffic caused by overlapping fixture requests
and global source traffic. The zero-source-byte assertion remains strict for
fixture traffic. This is observer instrumentation only; it changes no read,
write, admission or invalidation behavior.

Protocol run `c1233cb7349541de8e4a2033d8810594` verified that an unrelated
source read increases the global count while leaving the watched count
unchanged. It also verified 512-byte neighbor preservation and rejection of
overlapping/out-of-bounds observer ranges. Its normal read/write/cache checks
and cleanup passed. The native NTFS extent-query path was subsequently verified
in run `7eb6fed2054c479ca3c1c9db6d47bfe8`.

The separate process recovery run `1d9ecb6aa07d40ffaf33c73927eeabf9` passed,
recorded by `artifacts/iscsi-process-recovery-latest.json`:

- 256 acknowledged writes seeded a 16 MiB payload; a 4 KiB mutation was
  acknowledged and its client deliberately kept the session connected;
- the target child was forcibly terminated while that client was connected;
- the abandoned SSD cache was damaged after process exit;
- reopening the owned source created a different cache with empty mappings;
- all 256 blocks, including the mutation and unchanged neighboring bytes,
  passed the complete byte oracle after restart;
- the warm pass produced exactly 256 hits, 16 MiB SSD reads, zero source reads;
- owned files were removed, the listener closed, MSiSCSI and boot unchanged.

This proves process-boundary durability for acknowledged writes in the
disposable user-mode protocol test. Its Windows initiator counterpart is
separately verified above. Neither establishes abrupt power-loss behavior,
an existing physical-volume handoff or desktop activation. The later owned-VHDX
handoff is independently recorded above.

These disposable tests do not establish crash recovery or transparent
acceleration of the existing D: volume. A production source handoff and
reconnect/recovery design remains necessary before connecting that path to
desktop activation.
The partial-send issue has been fixed and tested using forced short-send sizes
and zero/invalid progress. Other transport recovery behavior, production service
ownership and desktop integration still require qualification. The desktop gate
remains intact.

## Dependencies

The private probe dynamically links `DiskAccessLibrary` 1.6.3 from the official
NuGet package and an adapted source build of `ISCSI` 1.5.6 (upstream commit
`8e31281a9c6d9f160c0ad996fd60358a0749b0e8`), by Tal Aloni, LGPL-3.0-or-later.
Original sources and packages remain intact under ignored `.tools/iscsi-research`.
`build-private-iscsi-transport.ps1` verifies the pinned server hash before applying
the adapters, and writes the exact compile inputs to `transport-build.json`.
Outputs go only to `artifacts/iscsi-probe`, not the desktop installer or release
qualification manifest. Project licensing is unchanged.

Sources: <https://github.com/TalAloni/iSCSIConsole>,
<https://github.com/TalAloni/DiskAccessLibrary>.
