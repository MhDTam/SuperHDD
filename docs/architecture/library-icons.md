# Local library icons

Library discovery embeds PNG data URIs generated from resources already on the
computer. No network requests or game launches are involved.

The previous inventory stopped after 80 successful icons and only searched for
executables in the installation root. It also did not inspect saved manual
folders outside the launcher/GAMES inventory. These limits hid game icons even
when the real executable was present under `bin/win_x64` or `Binaries/Win64`.

Discovery now merges Windows DisplayIcon references (including resource index),
Steam's local icon cache and Epic's launch executable. Failed references fall
through to a breadth-first installation search for ICO/EXE resources. The search
is bounded to five child levels, 192 directories, 4,096 examined entries and 32
resource extraction attempts per item. It skips child reparse points, large
asset trees and installer/crash-handler binaries. The installation root itself
can still be the application's active Dokan mount.

The extractor uses [ExtractIconExW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-extracticonexw)
for actual PE/ICO resources, releases both returned native handles and generates
a transparent 48px PNG. Invalid/absent resources retain the existing generic
game/app glyph. Broken image decoding in the UI also falls back to that glyph.

All inventory entries can receive an icon; there is no first-80 cutoff. Discovery
still has the existing 40-second subprocess deadline and three-minute in-memory
cache. Manual profile names/paths travel through a child-process environment
variable as JSON, without embedding them in PowerShell commands. Adding a manual
folder invalidates the discovery cache so the next UI read resolves its icon.
Fresh discovery icons take precedence over a stored stale icon; profile data,
cache configuration, game files and running cache sessions are not rewritten.

Validation: `scripts/test-library-icons.ps1` covers nested paths, valid PNG output,
quoted paths/resource indices, malformed resources, excluded installers, fallback,
depth/junction limits and 85 manual-folder icons beyond the former global limit.
`artifacts/library-icons/test-proof.json` records its result. The existing library
UI suite additionally checks valid image rendering and broken-image fallback.

The machine's final real inventory improved from 80 to 191 icons. One false
`steamapps` game row was removed (201 entries before, 200 after). Of 70 entries
classified as games, 67 have local icons. The three remaining rows are an empty
Free Chess directory, Steamworks Common Redistributables and Mortal Kombat 11
(its executable exposes no usable icon; its previous installer icon is now
rejected). These retain the game glyph. An intermediate 192-icon result included
uninstaller/helper icons; it is not the final result. This is icon discovery
evidence, not a game-performance benchmark.
