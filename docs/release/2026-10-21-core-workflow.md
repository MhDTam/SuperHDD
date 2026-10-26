# Core workflow update — October 21, 2026

The acceleration page now groups HDD selection, SSD/NVMe selection, cache size,
scope and cache management in that order. Runtime diagnostics and controls follow
the setup. The [product scope](../product-scope.md) records the user's requirements
and the remaining engine work.

Folder selection supports adding/removing full paths, per-source persistence,
source-volume checks, case-insensitive duplicate rejection and review of the
exact selected paths. Preferences are explicitly distinguished from cached files.
Both the UI and native runtime command contract refuse selected-only activation
because the volume driver cannot enforce it. Unknown scope variants and fields
are rejected rather than dropped.

The cache panel uses reported live capacity where available and connects clear
to the existing native invalidation command. It reports occupied bytes and file
inventory as unavailable. Neither lifetime fill counters nor configured budget
are presented as actual occupied space. Original source files are preserved.

Validation:

- `npm run build`: passed.
- `npm run test:ui`: 18 passed, including selection persistence, invalid paths,
  source switching, unsupported activation and cache clearing.
- The strengthened Arabic layout check also passed independently, including full
  sidebar visibility, document width and long paths at 1440/1050 pixels.
- `cargo test -p superhdd-platform --lib`: 35 passed. Four named-pipe tests first
  encountered access denial in the sandbox; the complete suite passed outside it.
- `cargo check -p superhdd-service`: passed.
- `npm run desktop:build`: passed; updated `target/release/SuperHDD.exe` and
  `target/release/bundle/nsis/Super HDD_0.1.0_x64-setup.exe` were produced.
- Workspace formatting check reports existing formatting differences in multiple
  unrelated files; no broad formatting rewrite was applied.

Tests use mocked runtime/hardware for UI behavior. They do not establish actual
selective acceleration, a cached-file inventory, production signing, physical
device qualification, or current-machine driver activation. No cache was started
and no source disk contents were changed by this update.
