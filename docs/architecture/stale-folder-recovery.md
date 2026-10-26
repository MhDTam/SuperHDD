# Recovery after a stopped Windows session

A Dokan folder mount can leave its NTFS junction behind when Windows or the owning supervisor stops without completing restoration. The backing directory still holds the authoritative game bytes, but Steam may report a missing executable or an unwritable installation folder because the junction targets a virtual volume that no longer exists.

The repair restores the recorded backing directory to its original location through the qualified `tools/loading-cache/session.ps1 -Action recover` path. Recovery checks source volume identity, original directory identity, the exact backing path, recorded mount tag and process creation identities. It removes the owned mount link, renames the original directory back, and invalidates disposable cache ownership. It does not replace game bytes or format storage.

`tools/loading-cache/recover-stale.ps1` scans only the owned library-runtime journals. Restored entries are skipped. A live supervisor or worker with the recorded creation time is preserved. An unreadable process identity, source mismatch, linked journal or other ambiguity leaves the paths intact and records the error in `artifacts/library-runtime/last-recovery.json`. An exclusive scan lock prevents concurrent recovery passes; each session also retains its own existing lock.

Before enabling a new cache session, the native backend registers a per-user logon recovery task. The task runs under the signed-in user with limited privileges, requires no stored password and only restores orphaned mounts; it does not start caches. Opening the desktop also registers and runs recovery in the background. Owned task names and descriptions are checked before replacement. Existing worker, session and launcher qualification hashes are preserved.

The regression script `scripts/test-stale-folder-recovery.ps1` uses a new private fixture and a private pool. It verifies preservation of a live session, the registered logon action after termination of the fixture's own supervisor, byte-for-byte restored data and refusal of an altered source identity. This exercises the logon action on demand without rebooting the user's computer.
