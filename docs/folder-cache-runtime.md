# Selected-folder cache runtime — October 22, 2026

The accepted free local backend reuses installed unmodified Dokan 2.3.1 binaries.
Both signatures are valid; the driver is signed by Microsoft Windows Hardware
Compatibility Publisher. No custom kernel binary is loaded. Dokan's driver and
library are LGPL; preserve the license obligations in any future distribution.

## Data and recovery

`tools/folder-cache/session.ps1` moves the selected directory to an owned sibling
on D, records its NTFS identity durably, and mounts `worker.cpp` at the original
path. Original data stays on D. Stop restores the directory and removes the SSD
cache. Never delete the `.SuperHDD-<session>` directory: it holds original files.
Close the selected game before starting or stopping its cache.

The worker uses 64 KiB blocks keyed by volume/file identity and offset, CLOCK
eviction, aligned unbuffered SSD I/O and SHA-256 per block. Writes invalidate
before modification and flush to D before success. Source read leases exclude
independent backing writers; conflicting opens bypass caching. Named streams
pass through uncached. Reparse paths and EFS data are rejected. Normal unlocked
BitLocker filesystem handles are used; the raw-volume planner is unchanged.

The 25 GB request rounds down to 24,999,952,384 cache-data bytes. There is no
dedicated RAM data tier; metadata, buffers and Windows' own cache still consume
memory. Sequential streams bypass admission after 8 MiB. Mappings are discarded
at worker restart and refill through reads.

`artifacts/folder-runtime/current.json` records source identity, exact mount tag,
paths and process start times. Worker failure triggers supervisor restoration.
A job kills the worker if its supervisor dies. The next recovery verifies
ownership before restoring; it refuses to overwrite a populated path or changed
identity. Stop in the desktop also recovers interrupted sessions.

The original host-launch method allowed the command runner's lifetime to end
both processes, leaving the mount inaccessible. The reported game path was
restored and all 136 file metadata records and the launcher hash matched.
The replacement `schedule-session.ps1` uses an owned, on-demand Windows Task
Scheduler task under the logged-on user's limited token, without a stored
password, elevation or a startup trigger. Its processes are outside the launching
command's job. The desktop itself can also be started through the separate owned
UI task. These tasks do not enable automatic startup or reboot recovery.
Manual recovery from the workspace:

```powershell
& .\target\release\examples\folder_client.exe stop
```

Only one selected D folder with an M cache is supported on this machine. Current
user control needs no UAC. Whole-volume requests retain their legacy safety gate.

## Evidence and limits

The manifest `artifacts/folder-cache/qualified.json` binds these locally checked
binaries to the native and supervisor proofs:

- Worker SHA-256: `CEA8B8D55E4FC814F4265FDDF9C65CBEF19B1E51CE37821267B617F35A507FE6`.
- Supervisor SHA-256: `A0B016E7C226987431F0A048EC75BEA2B71529E81A90DE9BE4F9E0D8BB9E2169`.
- Scheduled launcher SHA-256: `F82A4BC65FFFE7F5E65CD55EE67DA3E53B98A4AD8B7A828929CEE513C25C672C`.
- `host-proof.json`, run `f65374414edd47a69aa3e59df15e6e08`: host and worker
  excluded from a launching kill-on-close job; terminating that job left the
  selected fixture readable/writable and subsequent stop restored its data.
- `runs/92a05b880f7d4344b0128f839296854e/result.json`: 13 checks at 25 GB,
  including correct warm SSD reads, durable write invalidation, mapped writes,
  named streams, rename/truncate/delete, pause, clear and restoration.
- `session-proof.json`, run `5739a9e13ea64f93928b47fa1a297874`: normal stop,
  worker failure and supervisor termination followed by journal recovery.
- Rust cache core: 17 tests; platform: 41 tests under the real user. UI: 24 tests,
  plus two selected-folder cases after the final UI binding changes, including
  importing an already active session when the desktop opens.
- The old read-only probe's shutdown fault was fixed by keeping Dokan's DLL
  mapped until process exit. Eight alternating drive/directory cycles passed;
  historical failure records remain failed.

Actual game metadata, byte samples, warm SSD hits and restoration evidence are
recorded separately in `artifacts/diagnostics/sparking-zero-*-proof.json` when
run. These are local prototype checks, not a full filesystem conformance suite
or proof of compatibility with every launcher or anti-cheat. No actual game
launch or performance advantage over PrimoCache is established by these tests.
See [the comparison contract](performance-target.md).
