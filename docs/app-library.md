# Cache configurations and game library

The October 23, 2026 local build manages selected-folder caches on configurable
HDD/SSD pairs. It does not accelerate an entire raw disk. Existing game paths
stay the same; original files remain on their HDD, with synchronous writes.

## Configure drives

Open **Cache settings → Add configuration**, choose a healthy NTFS data volume
on an HDD, choose an SSD data volume on a separate physical device, and choose
1.1–25 decimal GB. Names are optional. **Refresh devices** discovers newly
connected storage. System/boot volumes, unidentified media, unhealthy,
read-only and offline devices cannot be selected for activation.

Each HDD volume has one configuration. Up to eight configurations and eight
simultaneously active folder sessions are supported. Several HDD configurations
may share an SSD; all their planned budgets must fit while preserving the SSD's
10%/10 GiB free-space reserve. Device IDs and volume GUIDs are saved and checked
again at activation. Reusing a drive letter cannot silently redirect the cache.

- **Edit configuration** changes its name or capacity. Live size changes resize
  only that configuration's workers. Stop it before changing either drive.
- **Stop configuration** checks that its applications are closed, restores its
  ordinary folders and releases their cache files. The configuration remains.
- **Delete configuration** confirms the operation, stops its caches and removes
  the configuration. Original folders and other configurations are preserved.
  Saved game profiles remain, stopped; add a suitable configuration to enable
  them again. Deleting a configuration does not format or erase a drive.

Games in a configuration split its capacity equally in aligned 64 KiB shares,
with at least 1 GiB each. Starting/stopping a game rebalances only that HDD's
configuration. A paused game retains its share. Stopping releases physical cache
allocation while keeping the configured budget for future use; delete or reduce
an unused configuration to reassign its planned capacity.

## Choose games and inspect cache

The **Library** reads Steam/Epic manifests, Windows installation metadata and
immediate `GAMES` subfolders on ready fixed local drives. It does not launch
installers or recursively scan entire drives. Search, type filters and a
configuration filter help select an item. Manual paths and a folder picker
remain available. A selected folder includes all descendants.

**My cache** shows managed profiles; **Installed games & apps** opens the catalog.
Unconfigured drives can be shown, with activation disabled until configured.
Each card shows its HDD/SSD pair, resident cached bytes, reservation and file
count. **Cached files** expands independently with search, size/name sorting
and 25-row increments. It uses relative paths; tooltips retain full paths.
Sizes describe resident portions, not original file sizes. Byte totals use
decimal B/KB/MB/GB/TB, matching capacity inputs.

Pause/resume and clear are independent per game. Clear discards that game's
cached copies. Stop restores the ordinary folder but keeps the game's cached
data in the shared pool for its next start; every file is revalidated against
its size and NTFS timestamps first, so files changed in between (for example by
a Steam update) are reread from the HDD. Removing a stopped profile discards its
cached data and preserves source files; an automatically discovered
installation may still appear in the catalog.

## Performance and diagnostics

One **Performance & diagnostics** page contains throughput, hit statistics and
explicitly dated benchmark evidence. Its expandable **Diagnostics** section
shows current engine/host state, counters and errors. Old Downloads and Gaming
pages and download IPC/startup are removed. No internet-speed or game-load-time
improvement is implied by storage correctness checks or old hardware benchmarks.

## Runtime and recovery

The new candidate is isolated in `tools/config-cache` and `artifacts/config-cache`.
Each application has its own current-user on-demand Task Scheduler host. Closing
the desktop does not kill hosts. Originals move only to a journaled owned sibling
on their HDD while the original path is mounted through the installed signed
Dokan runtime. EFS, reparse points, overlap and traversal are rejected. No
partitioning, boot-policy, pagefile or dedicated RAM data-tier change is made.

Journals and profiles remain under `artifacts/library-runtime` in this workspace.
New cache files are on the configured SSD under:

`<SSD>:\SuperHDD-Cache\<installation-owner>\<app-id>\<session-id>\cache.bin`

The owner directory has a protected current-user/SYSTEM DACL. Recovery validates
source identity, expected mount tag and exact owned cache path before restoration
and deletion. Volume GUID and disk ID checks guard changed/missing devices.
Reconnect the original storage to finish an interrupted recovery. At sign-in
the recovery task restores interrupted folders and then restarts the caches
that were active at shutdown; a cache you stopped stays off. Cached data from
before the restart is reused after validation. Earlier sessions with cache files
inside their journal directories remain recoverable during migration.

## Evidence

The qualified manifest binds the new worker, session controller and launcher.
The worker regression run `7538f9b5a83a4d80a347ab0f5601f3f5` passed 13 checks.
Host run `93fc700540504750846738795487afbb` passed eight checks, including real
D→M and E→F simultaneous mounts, independent pause/clear, retained data across
resize, normal stop, worker-crash restoration and supervisor-crash recovery.

`scripts/test-config-manager.ps1` tests configuration creation, independent
budgets, duplication/size/identity guards, live resize, stopped SSD changes,
sharing one SSD, stop/removal and original profile preservation. Its result is
`artifacts/config-cache/manager-proof.json`; only `passed: true` qualifies a run.
Run `c7f689ddc577406d87927860365520d8` passed all 12 manager checks on the
physical machine, using new bounded files on the existing D/E HDDs and M/F SSDs.
The final UI suite has 35 checks, including independent configuration controls,
capacity typing, merged diagnostics and Arabic desktop-width layouts.

The real game's new baseline and migration checks are saved under
`artifacts/diagnostics/sparking-zero-config-*.json`. Sources are inventoried
before migration, checked after restoration and checked again after activation,
including matching sample bytes and repeated unbuffered reads from SSD.
