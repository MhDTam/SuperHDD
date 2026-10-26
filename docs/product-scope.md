# Super HDD: core workflow

## Current accepted scope — October 22, 2026

The user subsequently accepted selected game/program folders as the permanently
free local solution and selected `D:\GAMES\DRAGON BALL - Sparking! ZERO`.
This supersedes the whole-volume-first decision recorded below.

The current backend reuses installed signed Dokan binaries and mounts the cache
at the folder's original path. Originals stay on D in an owned sibling directory;
a bounded ordinary file on M holds cached copies. Start, pause/resume, clear,
resident file inventory and stop/restore are connected to the desktop.
Writes remain synchronous to D. No dedicated RAM data tier, repartitioning,
pagefile change or boot-policy change is required. It does not cache all of D.

See [current implementation, evidence and limitations](folder-cache-runtime.md).
The October 23 library update adds installed-game/application discovery, manual
subtree selection, per-application controls and a shared 25 GB budget for up to
eight independent sessions. See [library behavior and validation](app-library.md).
The subsequent configuration update replaces the fixed D/M pairing with saved
HDD/SSD configurations, each with its own capacity up to 25 GB and independent
stop/removal. Performance and diagnostics now share one page. This supersedes
earlier single-pair/global-budget descriptions; details and current evidence are
in the library document linked above.
The unsigned v8 driver remains preserved and inactive on the host. No advantage
over PrimoCache has been measured. The remaining sections document the earlier
volume backend and must not be read as the selected-folder backend's status.

The user's October 21, 2026 clarification defines the core product:

1. Select the HDD data volume to accelerate.
2. Select the SSD or NVMe data volume that holds the cache.
3. Choose the cache capacity.
4. Inspect what occupies the cache and clear cached copies.
5. Optionally restrict caching to specific games or programs on that HDD.

These controls belong in one acceleration page. Downloads, benchmarks and
diagnostics are secondary tools and do not substitute for these requirements.

## Current implementation

The latest October 22 direction adds investigation of a reusable signed filter
with a target of outperforming PrimoCache. Permanent free local use remains a
constraint. This does not establish a chosen SDK, a license purchase, host
activation, or measured superiority. See [the comparison contract](performance-target.md).

A subsequent [Dokan directory-mount experiment](dokan-directory-cache-proof.md)
passed for a new read-only fixture at a D directory path, using the installed
signed runtime and an M cache file. This is an alternative selected-folder proof,
not whole-volume acceleration or an existing-folder migration. A separate
S-mode process failure remains unresolved. No production backend switch follows
from the experiment alone.

### Primary execution architecture — October 22 clarification

The user explicitly selected the PrimoCache-style storage-block architecture.
For the current local milestone, D stays mounted at its existing paths and the
v8 kernel storage filter serves eligible reads from a 25,000,000,000-byte SSD
cache budget on M. Applications must not need a second drive letter, alternate
folder, or launcher. Writes remain write-through to D with cache invalidation;
there is no dedicated RAM data-cache tier. Do not introduce deferred writes as
part of this milestone. Preserve the pagefile and host boot-security settings.

Dokan's successful read-only virtual-drive proof remains historical evidence,
not the selected execution backend. The CBFS file-filter SDK evaluation is also
an alternative architecture and is not a replacement for this storage-block
milestone. Optional file-aware selection/inventory work below must not be
presented as a prerequisite for basic whole-volume acceleration.

The existing v8 candidate is implemented and VM-tested, not host-enabled. The
current read-only review is `artifacts/diagnostics/paging-local-readiness.json`:
payload, current source, and VM proof hashes match; buffered/mapped SSD I/O was
observed in that proof. Windows kernel-policy verification still rejects the
unsigned physical driver, and physical-machine qualification is not recorded.
Do not bypass those checks or describe D as accelerated.

Drive discovery, capacity planning and the existing gated service controls are
connected to native APIs. The acceleration page now includes folder selection
and cache management. Folder preferences persist per stable source disk and
volume identity. Full subfolder paths must be on the selected HDD; duplicate,
relative, root and traversal paths are rejected. The list is explicitly a draft
selection, not an inventory of cached files, and does not assert folder existence.

The request carries selected folders through planning and review. The current
block/volume engine cannot enforce them, so both the interface and native command
validation reject selected-only activation, including before UAC and service
setup. Unsupported policies must never fall back to whole-volume caching.

The cache panel uses live capacity when the driver supplies it. Used space and
file names remain unavailable because the current telemetry does not expose
them. Cumulative fill bytes, configured capacity and total backing allocation
are not substitutes for current occupancy. Clear uses the existing service
invalidation command and preserves original HDD files. Controls remain gated by
the actual runtime state and release qualification.

## Remaining functional acceptance criteria

- Implement and validate a file-aware allowlist. Reads outside selected folders
  must bypass the cache. Include game assets and program data within the chosen
  folder tree, not just the executable. Handle writes, renames, deletion,
  replacement, file identity changes and reparse points without stale reads or
  caching outside the selected source.
- Track live resident cached bytes by file and application/folder, with a paged
  inventory. Report original HDD path and current cached size. Do not count
  uncached selected files as cache occupants.
- Show total live occupied/free cache space and offer clearing all copies. Add
  per-item clearing only once the engine can invalidate those entries reliably.
  Clear must leave source files intact and subsequent eligible reads may refill.
- Validate selected-only enforcement, byte correctness, write invalidation and
  whole-cache clearing in an isolated test environment before exposing activation.
- Complete the existing physical-machine and signed-runtime qualification.

The native cache budget remains bounded by the existing 25 GB development limit;
this change does not raise that engine limit or install/activate a storage driver.
