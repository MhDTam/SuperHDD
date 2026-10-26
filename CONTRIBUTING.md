# Contributing to SuperHDD

Bug reports, documentation improvements, and code contributions are welcome.
Open an issue describing the expected behavior, actual behavior, and steps to
reproduce. Remove personal paths, disk serial numbers, and account information
from diagnostics before sharing them.

For code changes, create a branch, keep the scope focused, and open a pull
request explaining the behavior and validation. Contributions to project code
are provided under the repository's MIT license.

Follow the Windows build instructions in [README.md](README.md). For frontend
changes, run `npm run build` and the relevant Playwright tests. For Rust changes,
run the relevant Cargo tests. Storage and recovery changes also need the
appropriate integration checks on disposable fixtures.

The integration scripts include machine-specific drive letters, SDK versions,
and lab assumptions. Read and adapt them before running. Keep generated runtime
manifests, cache files, VM images, credentials, and local diagnostic reports out
of commits. Do not mark a runtime qualified without running its required checks.
