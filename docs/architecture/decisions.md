# Architecture decisions

## 2026-10-17: authoritative source and separate implementation boundaries

1. The original HDD is authoritative. No asynchronous write-back is implemented.
2. No dedicated RAM cache exists. Rust `FileCache` retains only mapping/eviction
   metadata and one 64 KiB transient buffer per serialized read operation. The
   kernel implementation uses a transient aligned 64 KiB buffer per queued read.
3. A volume-level filter is the experimental interception candidate: it can see
   block writes beneath file-system caching, including paths a read-only file-copy
   utility cannot observe. A file-system minifilter was considered, but coherent
   treatment of paging/mapped I/O and file lifecycle would require a separate
   design and validation matrix. No universal interception guarantee is made.
4. The kernel filter uses direct-mapped metadata and a volume-wide generation.
   Invalidating all entries on writes sacrifices hit rate for a simpler coherence
   boundary. Rust's separately tested CLOCK algorithm is a user-mode harness;
   its tests do not validate kernel scheduling, physical I/O or cache coherency.
5. Driver cache backing is a new preallocated file. Existing files are never
   opened for truncation. Separate source/backing physical devices must be
   enforced by the lab controller before enabling its experimental attach path.
   That enforcement is NOT complete, so the attach path stays release-gated.
6. The native cache index is invalidated on attach/reboot. Persistent metadata is
   deliberately unsupported until offline writes and source generations can be
   proven safe. The UI never claims persistent caching is active.
7. All normal discovery and downloads run without elevation. The optional Windows
   service exposes read-only discovery/planning/telemetry over a local named pipe.
   IPC rejects remote clients, limits request/reply sizes and times out stalled
   connections. The UI checks the peer PID against the actual SCM service PID.
   No URL, shell command or arbitrary file-write capability is accepted by it.
8. Download workers are current-user operations rather than LocalSystem jobs.
   Staged bytes are quota-reserved under a shared lock before writes. HTTP resume
   requires a strong ETag and matching Content-Range; otherwise the payload is
   restarted. Copies are hashed again at destination before atomic hard-link
   publication. These checks verify transport/copy consistency, not publisher
   authenticity. Existing files are never overwritten.
9. Downloads default to LocalAppData. A later update adds a persisted dedicated
   staging-folder choice on a healthy NTFS SSD. Switching is blocked until the
   existing list is empty and never moves published files. Actual canonical-root
   space checks replace the earlier LOCALAPPDATA-based assumption. Offline saved
   locations fall back with a warning. Schedule/idle policies and service-owned
   downloads are not implemented; interrupted jobs resume as paused.
10. PrimoCache is a private development reference. Its settings card and desktop
    inspection command were removed; no vendor integration is advertised. It is
    not relabelled as a working native engine. Its L2 design requires independently
    validated storage; no repartitioning or vendor-driver installation occurs.

## 2026-10-18: download process and publication recovery

- A per-user/session Windows named mutex prevents duplicate desktop instances;
  a second launch restores the existing window. An exclusive file lock separately
  protects the shared download directory, including across retained manager clones.
- Download manifests are bounded and validated before recovery. Strong-ETag resume
  handles ignored ranges, changed validators and a one-time clean restart on 416.
  At most four workers may run, including workers resumed from saved manifests.
- Persist the verified staged hash and publication intent before destination copy.
  After interruption, rebuild the exact owned pending copy, or accept an already
  published file only if its hash matches. Retain staged bytes on failure.
- Persist completion before reclaiming owned temporary payloads, then expose the
  completed state under the same lock. Startup reclaims any completed leftovers.
  Removing a list entry never deletes the published destination file.
- Downloads default to a 1 GiB staging quota. This fits this host's C: reserve;
  the previous 20 GiB default was rejected by the real free-space safety check.
- Dialogs isolate background controls, trap keyboard focus, close on Escape and
  restore focus. Cache-capacity text remains a string during editing so entering
  125 does not become 0125 after clearing the field.
- Only one installer format is shipped. Tauri's optional bundle-type mutation is
  disabled so the installed and standalone binaries are directly hash-comparable.

## 2026-10-18: kernel v2 source hardening

- Epoch invalidation brackets source writes, flushes and storage-control requests.
  Cache admission/hits require no active mutation and an unchanged generation;
  generation exhaustion disables caching. These helpers are shared with host C tests.
- One dedicated reader thread processes at most 16 pending requests, each at most
  1 MiB. A transient aligned 64 KiB buffer serves 4 KiB-aligned partial blocks.
  Failed speculative source reads retry only the requested range. Paging and
  unsupported MDL paths pass through. PnP/power/shutdown disable caching until reattach.
- Lab-only manual detach drains request rundown and joins the reader before detach.
  Prompt cancellation and lower-stack hangs are unqualified; use only a quiesced
  disposable raw source. This is not a complete mounted-volume PnP integration.
- Lab attach requires distinct single-extent file-backed VHD devices, an unmounted
  raw source and new exclusive NTFS backing below a pinned non-reparse parent.
  Capacity is capped below 25 decimal GB; backing EOF and reserve are checked.
  Virtual extents do not prove separate physical host media or nonnested backing.
- Protocol v2 is 128 bytes. Typed Rust status validates the version, flags and limits.
  An administrator-only developer CLI supplies lab attach/pause/clear/detach;
  no privileged mutation is added to the desktop or public service pipe.
- Both variants build and pass MSVC project-source static analysis. External SDK/WDK
  definitions are excluded using `/external:I` and `/analyze:external-`; no project
  diagnostic is suppressed. These checks do not replace driver runtime qualification.

## 2026-10-19: native-only runtime and read-only v4 telemetry

- The user's explicit native-only choice supersedes any fallback proposal.
  PrimoCache is a conceptual reference, never the runtime backend. Remove its
  disk-path/shortcut discovery, snapshot field and unused inspection function.
  No vendor driver/service/task was installed or activated; the downloaded
  reference installer was deleted. Historical research metadata is not a product
  dependency and is not bundled.
- Prefer Super HDD's native-v4 status endpoint with a read-only handle. Fall back
  to our v2 endpoint only on Win32 file/path-not-found. Do not hide access denial,
  short replies, invalid flags/identities/budgets or protocol mismatch.
- Keep the existing kernel wire ABI intact. An optional typed JSON diagnostic
  object carries native source/cache disk numbers, total allocation, paging hits,
  cancellations and source-revalidated record counts. Detached disks serialize as
  null, not an invented disk number zero. The UI retains explicit lab status.
- The desktop and interactive-user service pipe expose observation only. Native
  configuration stays in the administrator-only disposable-VM controller. Physical
  source gating, production qualification and Microsoft signing are unchanged.
- `test-native-bridge.ps1` prepares only the existing network-isolated VM and
  installs a demand-start Super HDD service in a protected guest Program Files
  directory. `native_bridge_probe` uses the exact desktop telemetry function and
  SCM-PID-authenticated service route, compares ordinary unbuffered file bytes,
  checks real SSD-hit changes and detached state, then cleans its owned fixture.
  Its first launch was canceled, but the subsequent ordinary Windows elevation
  succeeded. A real run exposed a client ERROR_PIPE_BUSY race after the previous
  request's ACK; the client now retries only that error for at most one second.
  A real pipe test verifies the timeout and 32 successive framed requests. The
  corrected service/probe then passed the full VM contract and shut the VM down.
  Missing/access failures and SCM peer authentication remain unchanged.
- The rebuilt desktop passed real native UI/download/Arabic checks, ordinary
  startup/single-instance checks and current-user installer preservation tests.
  Old kernel benchmark evidence stays bound to its unchanged kernel and lab tools;
  it does not establish runtime success of the newly connected service bridge.

## Build environment

- Visual Studio 2022 Community C++ toolset 14.44.35207.
- Windows SDK 10.0.26100.0.
- Microsoft.Windows.WDK.x64 NuGet 10.0.26100.6584, extracted under `.tools/wdk`.
- Project-local Rust stable MSVC; exact dependencies are in `Cargo.lock`.
- Frontend dependency versions are in `package-lock.json`.
- The user's current host reports Windows 10 Pro; Windows 11 has not been tested.

## Primary references

- https://learn.microsoft.com/en-us/windows-hardware/drivers/install-the-wdk-using-nuget
- https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/about-file-system-filter-drivers
- https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/nf-ntddk-ioattachdevicetodevicestacksafe
- https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/different-ways-of-handling-irps-cheat-sheet
- https://v2.tauri.app/start/prerequisites/
- https://www.romexsoftware.com/en-us/primo-cache/instructions-command-line-interface.html
- https://www.romexsoftware.com/en-us/primo-cache/terms-configuration.html
