# Super HDD

## Current local build — selected game folders

The user accepted permanently free selected-folder caching on October 22, 2026.
The current desktop uses the installed signed Dokan runtime to cache selected D
folders at their original paths, sharing up to 25 GB on M. The games/applications
library discovers installed entries, includes nested folders and provides
independent start, pause/resume, clear and stop/restore controls. Writes remain synchronous to D.
Boot security and the pagefile are unchanged. This does not cache the entire D
volume and no advantage over PrimoCache has been measured.

The current interface focuses on Library, configurable HDD/SSD pairs, the combined
Performance & diagnostics page, Storage and general Settings. Each configuration
has an independent capacity up to 25 GB; several configurations can share one
SSD within its available space. Downloads and the placeholder Gaming page
have been removed, including desktop download-manager startup and commands.
The older implementation notes below retain their historical scope.

October 26, 2026 cache-engine update: the first load of a game now fills free
pool space, cached data survives stopping a game, restarting the worker and
rebooting (each file is revalidated against its size and NTFS timestamps before
reuse), and caches that were active at shutdown restart after sign-in. Pool
lock contention and full-index scans on writes were removed. See
[the loading replay](benchmarks/2026-10-26-loading-replay.md) and
[the engine notes](architecture/loading-priority-cache.md).

Run `target\release\SuperHDD.exe` from this workspace. See
[current scope](product-scope.md) and
[games/applications library](app-library.md), plus
[runtime, recovery and evidence](folder-cache-runtime.md).
The older volume-filter development history below is preserved for reference.

Windows desktop development build using Tauri 2, React, TypeScript, Rust and a WDM
storage-filter implementation. The UI follows the supplied powder-blue, slate,
lavender and blush glass references; English and Arabic RTL are included.

The [core product scope](product-scope.md) follows the user's five-step
workflow: HDD, SSD/NVMe, cache size, contents/clear, and selected games/programs.
The acceleration page saves per-source folder preferences and shows cache
management. Selected-only execution and cached-file inventory remain unsupported
by the current volume engine; selected-only requests are rejected before activation.

The current development implementation is the [native v8 paging-capable storage
filter](native-paging-filter.md). It keeps the source mounted, reserves its
read resources before activation, and enrolls the SSD backing path for paging I/O.
The selected local target is PrimoCache-style block caching: existing D paths,
a 25 GB cache budget on M, and no alternate virtual-drive path for applications.
The isolated guest has demonstrated correct buffered and mapped reads served from
SSD under memory pressure, using new source/cache files on the real D/M parents.
The CNG SHA-256 update and current service/desktop passed a separate final SCM
and workload run, including automatic owner recovery and cold-boot cleanup.
Warm SSD sample medians improved from 449 to 222 ms for buffered reads and from
658 to 306 ms for mapped reads. These bounded guest measurements do not qualify
the physical host or guarantee application launch times.
The [v7 passthrough implementation](native-transparent-filter.md) and its
completed kernel/SCM proofs remain preserved as historical snapshots.

Physical Windows 10/11 activation is not qualified. The physical driver artifact
is unsigned, and development desktop builds refuse activation before changing
storage. No host filter is installed and D acceleration is not active. The
application does not allocate a dedicated RAM data cache or repartition a
physical disk. Earlier v4/v5 and loopback-v6 reports retain their original scope.

## What works

The [current-machine check](release/2026-10-19-current-machine.md) uses only
new bounded files on the existing D:/E: HDDs and M: SSD; no empty disk is required
for these scoped tests. Ordinary-file cache tests and the isolated v5 service
cycle with D:/M: backing pass, with payload cleanup and guest shutdown. This does
not enable transparent caching for host Windows applications.

The latest development update is [the direct-v5 report](release/2026-10-19-direct-v5.md).
Its current local evidence is `artifacts/direct-v5-evidence.json`; earlier reports
and payload hashes describe their dated snapshots. The current v5 VM data/service
cycle passed, including crash recovery, cleanup after reboot and final shutdown.

The [v4 source/lab report](release/2026-10-19-driver-v4.md)
It adds a locked RAW cache backend, paging reads, targeted write invalidation and
a persistent index with mandatory current-source revalidation. The isolated
Server 2022 guest runs a locally test-signed driver. Byte-correct ordinary/mapped
I/O, write invalidation, reopen/revalidation and corruption fallback are recorded in
`artifacts/raw-source-evidence.json`. This does not qualify or activate a physical
Windows 10/11 accelerator.

- Native Windows desktop application, five primary navigation pages and three optional tools.
- Real physical-disk and NTFS-volume discovery, stable device identity, free-space
  checks, and a conservative eligibility/sizing planner.
- A real Windows service host and bounded, local-only, read-only named-pipe API.
  The client authenticates the peer PID against the Windows Service Control Manager.
- A read-only native-v4 telemetry bridge with source/cache identity, budget,
  paging-hit and cancellation diagnostics. The current deployed-service VM proof
  passes with byte-correct ordinary reads, actual SSD-hit changes and detached
  identities. Short-lived pipe contention is retried for at most one second;
  missing/access errors and SCM peer authentication are unchanged.
- Current-user HTTP/HTTPS downloads, validator-based HTTP range resume, pause,
  bounded staging, single-process staging ownership, interrupted-copy recovery,
  destination SHA-256 verification, atomic no-overwrite publication on NTFS,
  and list removal that preserves the published file.
- A file-backed CLOCK cache algorithm, checksum recovery, invalidation, eviction,
  pause, resize, aligned unbuffered Windows I/O and a reproducible hardware-file benchmark.
- A WDM volume-filter source implementation with an SSD read-hit path, coarse
  write generation invalidation, conservative fallback, and a real IOCTL telemetry
  bridge. This is separate from the user-mode algorithm proof.
- Dated, explicitly scoped E:/D:-to-M: file-cache benchmark results in Performance.

## Build

The subsequent [storage-controller development](release/2026-10-19-storage-control.md)
adds an owned native VHDX container and physical backing-dependency checks without
repartitioning a user drive. The [cache-control follow-up](release/2026-10-19-cache-control.md)
adds native RAW bootstrap, a durable two-slot ownership journal, normal/crash
reopen, transactional session control and a gated developer service command.
The [managed-service VM proof](release/2026-10-19-managed-session.md) passes
the complete native create/read/pause/clear/stop/reopen cycle. The subsequent
[SCM recovery proof](release/2026-10-19-scm-recovery.md) adds protected
LocalSystem ownership, automatic service-crash restart/reconciliation,
restricted-user denial and foreign-session preservation, with 96 passing Rust
tests and byte-checked recovered SSD reads. The subsequent v5 implementation
connects gated desktop actions and runtime setup/removal; physical qualification,
signed runtime lifecycle validation and accepted kernel signing remain pending.

Node.js/npm, Rust MSVC, Visual Studio C++ tools and WebView2 are required. This
workspace has a project-local Rust toolchain in `.tools/`; it is intentionally
ignored by Git. A fresh clone may use an existing Rust installation.

```powershell
npm install
. .\scripts\enter-dev.ps1
cargo test -p superhdd-cache-core -p superhdd-platform
npm run build
npm run test:ui
npm run desktop
npm run desktop:build
cargo build --release -p superhdd-service
```

`npm run desktop:build` creates the native application and a development NSIS
installer. It does not install the service or experimental driver. See
`installer/README.md` before installing a service or preparing a qualified release.
The single-format build disables Tauri bundle-type binary patching so the
installed executable and standalone executable have identical SHA-256 hashes.

```powershell
.\scripts\build-driver.ps1
# Only for building source in a driver-development lab (does not load the result):
.\scripts\build-driver.ps1 -ExperimentalLabBuild
# Separate mounted-volume lab candidate; never installs or loads the binary:
.\scripts\build-driver.ps1 -PnpSafeBuild
.\scripts\build-driver.ps1 -PnpLabBuild -Analyze
# Independent v4 locked-RAW-cache candidate, still disposable-VHD-only:
.\scripts\build-driver.ps1 -RawSafeBuild -Analyze
.\scripts\build-driver.ps1 -RawLabBuild -Analyze
.\scripts\test-driver-state.ps1 -RawStore
# New direct-volume candidate: compiles only; never installs on this host:
.\scripts\test-direct-policy.ps1
.\scripts\build-direct-driver.ps1 -Analyze
.\scripts\build-direct-driver.ps1 -VmTest -Analyze
```

The driver build uses a project-local official WDK NuGet package matching the
installed Windows SDK. See `docs/architecture/decisions.md` for the exact version.

## Run and inspect

`SuperHDDService.exe --diagnostics` prints live device discovery without installing
a Windows service. Browser preview is `npm run dev`. Add `?demo=1` only for a
clearly labelled visual preview with sample devices and charts. Ordinary browser
mode does not invent live hardware; use `SuperHDD.exe` for actual discovery.

Downloads run as the current user. Staging defaults to the app's LocalAppData
directory. Downloads settings can select an existing dedicated folder on another
healthy NTFS SSD; the canonical selected folder and its actual free-space reserve
are rechecked before creation. The choice persists. Clear list entries before
switching; published files are preserved and existing jobs are never migrated or
discarded. If a saved location is unavailable, the default is used with a warning.
Choose an existing NTFS HDD destination folder. The destination is never overwritten.
The current worker lifetime follows the desktop process; closing the application
interrupts work, and reopening recovers it as paused. The UI is not a hidden
permanent background process.

## Critical remaining release work

The mandatory transparent SSD hit/physical HDD-I/O-avoidance milestone is **not
verified**. Neither a passing algorithm harness nor a compiled `.sys` proves it.
Driver signing, isolated VM/disposable-volume validation, paging/PnP/power/removal
testing, performance evidence, recovery testing and the matrix in
`docs/release/validation.md` are release blockers. Persistent cache retention,
online kernel resizing, supported gaming policies, automatic updates and a
combined signed-driver installer are unsupported in this build.

PrimoCache is a local development reference, not an advertised app integration.
Its UI card and desktop inspection command were removed at the user's request.
No active vendor cache driver was found during the audit. No PrimoCache driver is
redistributed and no license, security setting, RAM tier or disk partition was changed.

The [2026-10-18 audit](benchmarks/2026-10-18-audit.md) records controlled
unbuffered file-cache tests on the user's actual HDDs with less than 25 GB of
temporary M: SSD backing. Payload files were released after validation; this did
not enable transparent system acceleration. E: is classified using a narrow
manufacturer-model fallback when its Windows/ATA descriptors are unavailable.

## Sources and evidence

- `docs/release/status.md`: precise build/test/qualification status.
- `docs/release/2026-10-18-final-audit.md`: final reliability audit and remaining blockers.
- `docs/release/2026-10-18-driver-v2.md`: subsequent driver hardening and SSD-staging update.
- `docs/release/2026-10-18-driver-v3.md`: separate mounted-NTFS/PnP candidate, cancellation and ordinary-file lab tools; not runtime-qualified.
- `docs/release/2026-10-19-driver-v4.md`: locked RAW backend, source-revalidated persistent index and isolated guest workflow; no production qualification.
- `docs/architecture/decisions.md`: architecture and scope decisions.
- `docs/security/threat-model.md`: local trust boundaries and outstanding audits.
- `docs/design-system/README.md`: tokens and original asset provenance.
- `output/playwright/`: actual browser screenshots and test report.
- `artifacts/driver/`: compiled safe-gated driver; never installed automatically.

No benchmark multiplier or production-performance claim is made.
