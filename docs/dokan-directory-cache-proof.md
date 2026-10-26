# Free signed runtime: cache mounted in a D directory

**Superseded experiment:** the user subsequently accepted selected folders.
The current read/write runtime, migration, recovery, 25 GB proof and DLL lifecycle
fix are documented in [the current runtime](folder-cache-runtime.md). Historical
failures below remain recorded and describe the earlier implementation.

Date: 2026-10-22. This is a bounded alternative architecture experiment, not a
replacement of the selected whole-volume v8 backend or activation of user folders.

## Result

The existing Microsoft-signed Dokan 2.3.1 runtime successfully served a new,
read-only 16 MiB fixture through a directory mount on D, backed by a 64 MiB SSD
cache file in the M workspace. No new driver installation, drive-letter change,
pagefile change, firmware operation or boot-policy change was needed. No S drive
was required by the directory-mode run. Source data remained on D.

Dokan's [upstream header](https://github.com/dokan-dev/dokany/blob/master/dokan/dokan.h)
documents an empty NTFS directory as an alternative to a drive-letter mount.
Its [repository licensing section](https://github.com/dokan-dev/dokany#licensing)
identifies the driver/library as LGPL, rather than a time-limited commercial SDK.
The experiment uses the already installed unmodified signed binaries; this does
not grant a signature to modified kernel code.

## Changes and evidence

- `tools/dokan-probe/bridge.cpp`: added a directory-mount entry point restricted
  to `D:\SuperHDD-DokanDirectory-<32 lowercase hexadecimal characters>\visible`.
  Rejects nonempty folders and reparse-point parents/targets. Existing S mode is
  retained. Unmount closes only the handle of the owned instance.
- `crates/cache-core/examples/dokan_cache_probe.rs`: optional `--directory-mount`
  mode, sharing existing cache, integrity and filesystem-read checks.
- `scripts/test-dokan-cache.ps1 -DirectoryMount`: verifies the installed runtime,
  separate physical source/cache identities, source hashes, cleanup and boot
  state. Records process exit codes for future failures.

Successful directory-mode runs:

- `artifacts/dokan-local-24d72f3228004d1e8654a36d7fd87195.json`.
- `artifacts/dokan-local-819f292e67784f3a96ab19489ae4f729.json` (after Rust formatting).

Checks passed: aligned unbuffered reads, correct bytes, increased SSD-cache read
and hit counters without additional source-API reads on the warm trace, mapped
reads, concurrent readers, rejection of writes, exclusion of external source
writers, pause/bypass, clear/refill, instance closure, and owned-file removal.
There is no dedicated RAM data-cache tier. These counters do not constitute a
device-level ETW trace or a PrimoCache comparison.

Build validation: native bridge compiled with `/W4 /WX`; the release Rust example
built offline; example Clippy with `-D warnings` passed.

## Regression failure retained in the record

Immediately after the final directory test, one S-mode regression run
(`25434218a02640719ce5a4a0f9ebf566`) exited without producing its inner result.
Its log was empty; the old wrapper did not preserve the exact native exit code.
The outer record is failed and must remain failed. The cause is unresolved.
The wrapper now records exit codes. A later isolated S-mode run
(`829b43c613204b5fbacd8a71b5ef4c74`) passed, but does not erase the earlier failure.

The failed run's two temporary files were subsequently removed after checking
exact resolved paths, expected lengths, non-reparse attributes, exclusive access
and the source's deterministic SHA-256. Separate cleanup evidence:
`artifacts/diagnostics/dokan-directory-failed-run-cleanup.json`.
No recursive removal was used. Both directory-mode runs removed their own files
and mounts automatically. Reliability is not yet established for real folders.

## What this changes, and what it does not

An application need not use a different drive letter: a virtual filesystem can
be mounted at a directory under D. In a future selected-folder design, the
original visible folder path could become the mount point while source files
are accessed through a separate backing location on the same HDD. That migration
has NOT been implemented or tested with an existing user folder. The experiment
creates an empty test mount and a separate new backing fixture from the outset.

This does not intercept the whole D volume, its pagefile, raw disk access, or
files outside the mount. It is a file-system proxy, not a storage-class filter.
The proof deliberately rejects writes and exposes one fixture; it is not a
usable read/write game-library filesystem or an active 25 GB cache.

Before any existing-folder migration: implement multi-file operations, stable
identity/ACL/stream semantics, concurrent write-through invalidation, mapped-write
coherence, lifecycle and crash recovery. A source folder cannot be both freely
writable outside the proxy and blindly trusted as cached data. A missing service
must not leave the application's path inaccessible without a tested recovery.
Account for hard links/reparse points and launchers/anti-cheat compatibility.

Further performance work must follow `performance-target.md`. Small fixture
timings are diagnostic only: drive/controller caching, tiny working sets and
one trace cannot establish game-loading speed or superiority to PrimoCache.
The unresolved process failure must also be investigated before broader use.
