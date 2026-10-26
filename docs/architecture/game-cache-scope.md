# Game cache scope

Library game cards expose a **Cache scope** choice: **Game data only** or **Whole
game folder**. A running cache locks this selection until it is stopped. The
discovered data path is shown explicitly. Missing/ambiguous data directories
disable the data option; activation fails instead of falling back to the entire
game folder. Compatibility depends on the game and protection system.

Discovery conservatively recognises `Content\Paks` and `CookedPCConsole` beneath
the game root or one immediate child, excluding Engine/support folders. Results
are cached for 180 seconds, bounded to 512 entries, and refreshed by Refresh list.
Before activation, inspection rejects executable/script payloads, reparse/EFS
entries, unreadable trees and more than 20,000 entries. All intervening directories
must be ordinary directories. Unknown layouts retain manual folder selection.
This is folder discovery, not an attempt to identify every engine or anti-cheat.

The root game keeps its identity, name and icon. An internal data-folder profile
retains the existing source-path-derived identity and session/recovery rules.
`cacheModes` preferences and `dataLinks` are optional maps in the existing store,
omitted when empty. Existing standalone data profiles at the discovered exact path
are adopted for display without remounting. The library presents one game card;
its statistics and controls target its active data worker. Source configuration
and shared pool capacity do not change. Parent pause/resume/clear/stop work on the
child worker; remove cleans both stopped profiles. Overlap guards remain active.
The process guard includes registered game ancestors of selected subtrees.

Data-only mode is suggested for newly discovered games with a known data folder.
An already active whole-folder session keeps its scope. Changing scope never
unmounts a running session automatically. Full-folder mode remains an explicit
choice and explains that some anti-cheat systems reject mounted executables.

Native validation: `scripts/test-game-scope.ps1` passed 16 checks using a new
256 KiB D-drive asset fixture and the existing shared pool. It verified parent
control routing, one-card display in both modes, ordinary executable location,
live-change refusal, missing/changed-target refusal and byte-for-byte restoration of the
original library store. No actual game session was stopped. The 15 library UI
checks cover selection, locked controls, missing targets and Arabic layouts at
1050/1440 px. Native asset-discovery tests cover executable rejection, ambiguity,
empty data and outside paths. Worker/cache file formats were not changed.

## Loading-performance evidence

The sampled PUBG asset-worker session had 4,803,948,192 useful SSD-served bytes
versus 22,869,327,567 source bytes: about 17.4% by payload. Its request-hit count
was much higher proportionally. Internal cache API reads totalled 43,300,057,263
bytes (about 9x useful SSD payload), because integrity checks read the stored
range, which can be larger than a small requested range. API bytes are not a
measurement of physical SSD traffic; Windows may serve them from its own cache.
These counters cover the session, not isolated loading-screen intervals.

This release adds scope/compatibility controls. It does not change the cache
admission policy or integrity read amplification, and does not claim a loading-
time or FPS improvement. Reducing that amplification needs a separate validated
engine change and a controlled repeated-scene comparison. Neither data scope nor
a high hit percentage guarantees every game's protection accepts the mount.
