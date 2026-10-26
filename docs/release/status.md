# Validation status — updated 2026-10-22

The current development path is the [v8 paging-capable filter](../native-paging-filter.md).
Its pre-CNG VM run demonstrated buffered and mapped SSD paging hits with correct
bytes under low available RAM. The CNG optimization, v8 LocalSystem owner and
desktop integration have dedicated evidence in
`artifacts/runtime/kernel-lab/paging-service.json`. Run
`405300c0a94349fa807c3bb2c732a625` completed all SCM/application checks and
cold-boot cleanup. Warm buffered/mapped sample median speed ratios were 2.02/2.15.
The candidate manifest pins the exact tested service and driver hashes.
Rust workspace tests pass (121) and current UI tests pass (23). The physical
image remains unsigned and host D acceleration remains inactive.

The following v7 section describes its completed earlier snapshot.

The current development path is the [native v7 storage filter](../native-transparent-filter.md).
The isolated kernel and SCM tests demonstrate ordinary SSD cache hits while a
real Windows page file remains on the source, including write oracles and owner
crash recovery. New desktop starts select this path. The physical driver is
unsigned, host activation is disabled, and D is not accelerated. Paging reads
intentionally pass through. See `artifacts/transparent-session.json` and
`artifacts/runtime/kernel-lab/transparent-service.json` for the bounded evidence.

The following v5/v4 sections describe their historical snapshots.

The current candidate is [direct v5](2026-10-19-direct-v5.md), with direct-source
policy, protected service ownership, gated desktop actions and runtime setup/
removal source. It passes 103 Rust tests, 15 browser tests, 44 policy assertions
and physical/VM kernel static analysis. Its current evidence is
`artifacts/direct-v5-evidence.json`. Physical Windows 10/11 qualification,
Microsoft signing and signed installer lifecycle tests remain pending. The
first v5 VM runtime probe passed but its wrapper cleanup failed. The subsequent
current-binary run completed all 13 checks and confirmed cleanup after reboot,
guest shutdown and zero assigned guest memory. This is recorded separately in
that evidence and does not establish physical-cache power-loss qualification.

The dated results below describe earlier source/binary snapshots. They do not
automatically qualify the newly changed platform, service or desktop binaries.

The earlier [SCM recovery update](2026-10-19-scm-recovery.md) passes 96 Rust
tests, Clippy and desktop integration checking. The actual native LocalSystem
service now proves protected ownership, automatic SCM restart after process kill,
exact-session reconciliation, source revalidation and byte-correct SSD reads in
the isolated signed-driver VM. Restricted-user access is denied and a foreign
kernel session is preserved. The previous native lifecycle also remains covered.
All run-owned payload disks, SCM service and protected configuration were removed;
the VM is off with zero assigned memory. Physical-source support, production
desktop activation, installation/rollback, power-loss/cold-boot qualification and
accepted kernel signing remain pending. The frontend is unchanged from its preceding passing build.
Earlier counts and scoped results below describe their dated source snapshots.

Current v4 source/lab work is described in
[the dated report](2026-10-19-driver-v4.md). The source passes 60 Rust tests,
188 shared-state C assertions and 124,814 RAW-format/interop C checks.
A network-isolated Server 2022 guest has booted and run a locally test-signed
kernel driver. Byte-correct ordinary/mapped I/O, reopen/revalidation and corrupt
payload recovery have passed in that lab. Current binary-bound results are in
`artifacts/raw-source-evidence.json`. No production acceleration is activated.
The final 512 MiB HDD-to-NVMe lab trace measured 139.95 IOPS paused,
120.67 cold and 1,613.88 warm (11.53x). All bytes matched; whole-D: physical
counter deltas during the warm window were zero reads/bytes, consistent with
8,192 driver SSD hits and no source reads. This is scoped lab evidence,
not production/game/Windows 10/11 qualification.
The v4 stress tool also passed bounded queue cancellation, clear/pause/disable
with reads pending, and independent writes with full-block checks after completion.
Those six phases passed under standard Driver Verifier and with Force Pending I/O
enabled on the disposable guest driver. Every successful read in the first five
phases is fully compared; overlapping-write reads compare immutable tails and
64 later reads check the whole updated block. Active-child cancellation, low-memory,
power/removal and long-duration qualification remain pending.
The final normal-driver run passed the same six phases. The VM then shut down,
assigned guest memory reached zero, and both stress and release evidence audits
passed with a fresh final-host snapshot. Earlier local elevation cancellation is
resolved; the production qualification blockers remain.
The desktop was rebuilt on 2026-10-19 with read-only native-v4 telemetry support.
It prefers the current endpoint and preserves the legacy-v2 fallback only when
the current device is absent; malformed or inaccessible current responses surface
as errors. No privileged configuration command was added to the desktop or public
service pipe. The deployed service-to-v4 VM probe now passes with byte-correct
ordinary reads, actual SSD-hit changes and detached/null identities. A first real
execution exposed ERROR_PIPE_BUSY between successive requests. The client now
retries that transient error for at most one second, without changing peer
authentication or hiding absent/access errors. Historical kernel probes and driver
benchmarks remain separately bound to their exact driver/tool hashes.

The current source passed 60 Rust tests, Clippy, 12 browser tests including mocked
v4 Arabic telemetry, 18 actual WebView2/native checks, normal startup/single instance,
and current-user install/start/uninstall with equal executable hashes. Vendor path
discovery and the unused vendor inspection function were removed. PrimoCache is a
conceptual reference only; no vendor driver, service or cache task backs this app.

This file distinguishes source implementation from runtime and hardware proof.
No production-ready or transparent-acceleration claim is made.

The subsequent [storage-controller work](2026-10-19-storage-control.md) adds nine
tests, bringing the workspace total to 69. Its new native SSD VHDX creation,
physical backing verification and cleanup passed on the actual host without
source writes or filter enrollment. It remains separate from desktop/service
activation; the previously verified desktop binary and v4 kernel are unchanged.
The user confirmed that no Hardware Developer account or EV certificate is
available, and requires a free solution. The
[free-only signing findings](free-signing-research.md) distinguish conditional OSS
application signing from a sponsor actually submitting our kernel driver to
Microsoft. The latter has not been confirmed. An eligibility inquiry is prepared
locally and remains unsent; no source was published, license changed, service
purchased or signing policy altered. The earlier paid-EV inquiry is historical
research and is not the selected solution.

| Component | Implemented | Builds successfully | Runtime tested | Verified on hardware |
|---|---|---|---|---|
| React/Tauri UI, English/Arabic, light/dark, reduced motion | Yes | Yes | 12 browser tests, eight pages, three widths, keyboard, 2x DPI and mocked v2/v4 states | 18 current native checks plus startup/installer passed |
| Physical disks/volumes/identity/free space | Yes | Yes | Actual discovery on this Windows 10 host | Five physical devices enumerated; unverified media/encryption remains Unknown |
| Cache sizing and safety planner | Yes | Yes | Unit tests | Temporary reservations below 25 decimal GB; later released |
| Rust SSD-file cache algorithm | Yes | Yes | 12 unit tests and two real unbuffered HDD/SSD file benchmarks | Source API avoidance inside harness; no transparent-system proof |
| HTTP/HTTPS downloader and verified publication | Yes | Yes | 22 integration/location tests plus actual process-kill recovery | Actual M: staging to E: publication; no internet/game speed claim |
| Windows service host | Yes | Yes | Read-only pipe tested in-process | SCM installation/start/stop requires validation |
| Service client peer authentication | Yes | Yes | Local protocol rejection tested | Full service deployment/security matrix requires validation |
| WDM v2 raw filter and separate v3 mounted-volume PnP candidate | Source implemented; paging hits/persistence incomplete | Safe/lab builds and project-source static analysis pass | 118 shared-C host assertions; neither driver loaded | Requires signed per-device VM deployment, kernel validation and physical I/O proof |
| Separate v4 locked RAW backend | Mounted NTFS source, paging reads, source-revalidated index, targeted write invalidation | Safe/lab builds and project-source static analysis | Signed disposable Server guest probe; current exact-image evidence in raw-source-evidence.json | HDD-to-NVMe lab benchmark is recorded separately; physical production gate remains closed |
| IOCTL telemetry protocol and typed bridge | Read-only v4 preferred, absent-device v2 fallback | Yes | ABI validation, mocked v4 UI and actual SCM-authenticated deployed service/driver VM proof pass | Physical sources remain rejected; no production activation |
| Native-only runtime | Vendor UI, inspection function and path discovery removed | Yes | Current native snapshot has no vendor field; native UI has no vendor control | No vendor driver/service/task installed or activated |
| Owned SSD VHDX and backing-dependency controller | Independent native component; not connected to activation | Yes | Nine new tests and actual 256 MiB-budget M: container creation/topology/cleanup pass | D/M physical separation proven; E seek-penalty unknown; no physical-source caching or performance claim |
| Desktop NSIS development installer | Yes | Yes | Current-user install, installed startup and uninstall passed in isolated workspace | Standalone/installed SHA-256 equal; existing app-data fixture preserved. Driver/service deployment remains pending |

## Completed automated checks

The subsequent v3 source update is recorded in
`2026-10-18-driver-v3.md`: the current Rust source passes 54 tests, the C helpers
118 assertions. The desktop/installer/native UI evidence below remains for the
unchanged previously built application; the v3 lab is not connected to its UI.

- 12 cache-core tests, including a 4096-operation read/write/resize/eviction trace,
  unbuffered partial reads and write invalidation.
- 17 platform tests, including conservative WD model classification, a data partition
  on a system disk, native status ABI, exclusive desktop mutex and real local
  named-pipe rejection of privileged, malformed, oversized and stalled clients,
  plus v2 status/attach ABI, flags, capacity and path validation.
- 3 local HTTP integration tests, including actual download, pause, strong-ETag range resume,
  byte-correct verified destination, no overwrite, staged-payload removal, restart
  manifest recovery, quota rejection and interrupted-job restart. A verified copy
  was published onto the real E: HDD using a controlled destination override.
- 14 download recovery tests: validator changes, ignored ranges, 416, redirects,
  truncation, interrupted publication, corruption, no overwrite, root ownership,
  manifest validation, worker limits and safe list removal.
- 5 location tests cover persisted SSD choice, owner release, nonempty-list protection,
  offline/malformed settings fallback, reselecting fallback and actual-volume reserve.
- 12 browser integration tests: welcome/setup, capacity preservation, gated actions,
  all pages at 1440/1366/1050 widths, Arabic RTL persistence, real-vs-demo states,
  reduced motion, theme, dialog focus trapping/Escape/restore, 2x-DPI editing and
  explicitly mocked paused/bypass v2 and native-v4 Arabic telemetry states.
- Rust Clippy with warnings denied; frontend TypeScript production build.
- WDM safe-gated and experimental-branch compile/link with warnings as errors.

The disposable-file proof log explicitly says it does **not** verify transparent
caching or physical HDD I/O. Its timings must not be presented as HDD benchmarks.

## Requires validation / unsupported

The dated v4 report records scoped SSD-hit/HDD-read-avoidance and bounded
concurrency/cancellation/Verifier evidence. Complete kernel coherency, active-child
cancellation/removal/power behavior, backing-device separation, Microsoft production
signing and release qualification remain blockers. Native
online resize, persistent metadata reuse, supported game policies, automatic
updates and combined signed-driver installation are unsupported. The UI identifies
these limits rather than showing fake active switches or measured values.

The installer is a development desktop build. It does not install a service,
modify a partition, allocate a RAM tier, redistribute PrimoCache, or attach the
experimental filter. New isolated disposable test files were created on E:, D: and
M: during the hardware audit; benchmark payloads were later removed. Existing user
files, services and partitions were not changed.

The dated [hardware audit](../benchmarks/2026-10-18-audit.md) describes the separate
unbuffered E:/D:-to-M: harness. It is a real file-level HDD/SSD measurement, not a
transparent Windows accelerator or driver qualification result.

## Artifacts

- `target/release/SuperHDD.exe`
- `target/release/bundle/nsis/Super HDD_0.1.0_x64-setup.exe`
- `artifacts/driver/SuperHDDCacheDriver.sys` (safe gate, unsigned)
- `artifacts/driver-lab/SuperHDDCacheDriver.sys` (lab branch, unsigned; never load on user disks)
- `artifacts/rust-tests.log`, `artifacts/cache-algorithm-proof.log`
- `output/playwright/results.json` and PNG screenshots.

## Native startup smoke check

On 2026-10-17 at 18:54 Baghdad time, the final release executable was launched
without elevation. Its `Super HDD` window and running process were observed.
The application was left open for review. This verifies native window startup,
not native UI automation or kernel acceleration. Live disk discovery was separately
executed successfully through the release diagnostics binary. Evidence is in
`artifacts/runtime/native-startup.json` and `artifacts/runtime/live-disks.json`
(local machine data, excluded from Git). SHA-256 hashes of the application,
installer, service and two unsigned driver builds are in
`artifacts/release-sha256.json`.

## Native functional audit — 2026-10-18

At 06:30 Baghdad time, ten checks passed against the real release WebView2 app:
E: detection, NVMe art, readiness gating, saved benchmark loading, no PrimoCache
card, unsupported gaming disabled, UI download creation, pause/range resume,
SHA-256-correct publication on E:, and Arabic rendering. This was native IPC with
actual devices, not a mocked browser fixture. The audit found and fixed a
same-origin CSP omission in the packaged benchmark loader.

See `artifacts/runtime/native-audit-20261018.json` and
`output/playwright/native-{setup,benchmark,benchmark-ar,download}.png`. This does
not qualify the experimental kernel driver or establish transparent acceleration.

## Final reliability audit — 2026-10-18, 07:15 Baghdad

The updated release passed 43 Rust tests and 9 browser tests with no failed or
skipped cases. Clippy with warnings denied and the TypeScript production build
passed. The Vite build retains a non-fatal JavaScript chunk-size advisory.

Fourteen checks passed against the final native WebView2 build, including modal
keyboard behavior, actual E: publication, list removal preserving that file,
Arabic, saved benchmark loading and no script errors. A separate second launch
exited successfully while the first process remained the only desktop instance.

A 64 MiB worker was forcibly terminated with 8,912,896 bytes in its pending E:
copy. A new process recovered it without network access, verified SHA-256 and
reclaimed both temporary payloads. The completed manifest is now persisted before
cleanup, with the completed state exposed to observers only after cleanup is attempted.

The current-user NSIS package was installed into a fresh workspace directory,
launched, then silently uninstalled. Installed and standalone executable hashes
matched, a unique existing-app-data fixture survived, and test registration and
installation files were removed. This is desktop-only validation, not rollback
qualification for a service or active storage driver.

The final application was restarted normally and left open with the local
WebView debugging port closed. Artifact hashes were refreshed. Evidence:

- `artifacts/rust-tests-20261018-final.log`, `artifacts/clippy-20261018-final.log`
- `artifacts/ui-tests-20261018-final.log`, `output/playwright/results.json`
- `artifacts/runtime/native-audit-final-20261018.json`
- `artifacts/runtime/single-instance-20261018.json`
- `artifacts/runtime/download-crash-result.json`
- `artifacts/runtime/installer-test-20261018.json`
- `artifacts/runtime/native-final-startup-20261018.json`
- `artifacts/runtime/driver-environment-20261018.json`, `hyperv-query-20261018.json`
- `artifacts/release-readiness.json`, `artifacts/release-sha256.json`

**Production readiness is false.** No active cache driver is running. The custom
driver is unsigned and still lacks runtime qualification and required kernel
safety/deployment work. Windows reports this process as non-elevated; the Hyper-V
query is denied by the host authorization policy. No kernel signing, VM operation,
driver installation or security-policy change was performed. An ordinary user
code-signing certificate is not evidence of a production-qualified Windows driver.

The Arabic [final report](2026-10-18-final-audit.md) explains the verified features,
the benchmark boundary and what remains unavailable.

## Driver v2 and SSD-staging follow-up — 2026-10-18

See the Arabic [driver update](2026-10-18-driver-v2.md). Current checks: 51 Rust
tests, 11 browser tests, 87 assertions in the actual shared C helpers, both driver
builds and both MSVC project-source static-analysis builds. External SDK/WDK headers
are excluded from analysis; project diagnostics are not suppressed. Driver runtime
remains untested, unsigned and gated; normal mounted-volume/PnP attachment is not
implemented. Only the experimental unmounted raw-VHD path exists.

Eighteen native release checks passed, including protection of the now-low C:
free-space reserve, selecting M: for download staging, actual M:-to-E: HTTP transfer
with pause/resume/SHA-256, list removal preserving the destination, and persistence
of the M: selection across a real process restart. The test payload was removed;
the empty selected M: folder remains for normal downloads with a 1 GiB quota, not
a preallocated acceleration cache. No user files were migrated. The latest desktop
installer also passed installation, installed startup, hash equality and uninstall.

Current evidence: `artifacts/rust-tests-driver-v2.log`, `ui-tests-driver-v2.log`,
`clippy-driver-v2.log`, `driver-{lab-,}analysis-v2.log`,
`artifacts/driver-state-test/result.json`, and updated native/installer/runtime/hash
reports. `artifacts/runtime/lab-preflight-v2.json` records the host's non-elevated
process, unsigned driver and denied Hyper-V access. None of these checks establish
kernel cache-hit correctness, physical source-I/O avoidance or game acceleration.
