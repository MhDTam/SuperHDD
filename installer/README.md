# Development installer and qualified release boundary

The new v5 runtime scripts are `install-runtime.ps1`, `enroll-source.ps1` and
`uninstall-runtime.ps1`. They implement a separate fixed-path privileged runtime
candidate. They are not included in the desktop NSIS package and have not passed
signed physical-hardware installation/rollback/uninstall qualification.

`scripts/package-qualified-runtime.ps1` requires an exact Microsoft-signed v5
driver, bound Windows 10/11 hardware qualification evidence and a service compiled
with that driver's release hash. It emits the two hashes to embed in the desktop.
Without them the desktop and helper refuse activation before mutation. The
installer independently checks payload hashes and the hardware evidence. Its
source and native enrollment helper compile checks pass; no qualified package
has been assembled or installed on the host.

See [the v5 report](../docs/release/2026-10-19-direct-v5.md) for the exact runtime
test scope and remaining release requirements. Earlier results below apply to
their dated desktop-only packages.

The Tauri configuration builds a normal NSIS desktop installer. It packages the
unprivileged application only. It does not redistribute PrimoCache, install a
storage filter, repartition a disk or disable a Windows security feature.

`install-service.ps1` is an explicit administrator operation for the read-only
service. Use a release-built executable copied into Program Files with protected
ACLs; never point a SYSTEM service at the writable development workspace. The
service does not activate any cache or download as SYSTEM. Installer/service
runtime qualification must be recorded separately from successful compilation.

A production installer must additionally validate OS/architecture, verify signed
payloads, install only a qualified signed driver, handle restart/rollback, record
ownership of cache storage, and safely detach during uninstall. These production
steps are not shipped as a pretend working installer.

On 2026-10-18 the desktop-only NSIS package passed a real current-user install,
installed-window startup and silent uninstall in a fresh workspace test folder.
The installed executable matched the standalone SHA-256, an app-data preservation
fixture survived, and test registration/files were removed. The reproducible
preflight-protected check is `scripts/test-installer.ps1`; it refuses to replace
existing installation or startup registration and never installs a driver/service.
This does not validate rollback with active cache tasks or driver/service setup.
