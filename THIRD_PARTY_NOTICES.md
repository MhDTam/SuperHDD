# Third-party components

The MIT license at the repository root applies to SuperHDD project code.
Dependencies retain their own licenses; the root license does not relicense
third-party components.

- The current folder cache uses the separately installed
  [Dokan](https://github.com/dokan-dev/dokany) runtime and development headers.
  Its library and driver have their own LGPL license terms. Dokan binaries and
  SDK headers are not included in this repository. Preserve the upstream
  notices and comply with its terms when distributing a runtime package.
- JavaScript and Rust dependency versions are recorded in `package-lock.json`
  and `Cargo.lock`. Consult each upstream package's license before distributing
  a combined binary or installer.
- The `tools/cks-study` harness references separately obtained upstream study
  sources. Those sources and their local provenance records are not included.
- Local design reference images and exploratory artwork in `UI/` are excluded.
  The application assets under `apps/desktop/public/assets/` and native icons
  under `apps/desktop/src-tauri/icons/` are the project artwork used by the app.
