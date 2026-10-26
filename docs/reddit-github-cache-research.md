# Reddit and GitHub cache research — 2026-10-22

Target: the existing D paths, a 25 GB cache budget on the occupied M SSD,
Windows 10 build 19045, no dedicated RAM data tier, and no host boot-security
changes. This review executed no downloaded programs or setup scripts.

## Findings

### Nova Cache

[Repository](https://github.com/confeden/Nova-Cache)

The author describes a Windows minifilter plus Rust service with RAM/SSD tiers.
The README explicitly requires administrator privileges and Windows Test Mode
for its self-signed driver. It is a relevant implementation reference, not an
established answer to this host's signing restriction. No independent performance
or production-readiness claim was verified.

### Windows 10 Custom Kernel Signers

[Repository](https://github.com/HyperSine/Windows10-CustomKernelSigners)

This is a historical community method, distinct from Microsoft's currently
documented Windows 11 deployment procedure. It claims self-signed driver loading
with Secure Boot enabled. Its prerequisites include control of the firmware
Platform Key. Its helper changes Windows setup/product policy and its persistence
driver maintains that policy; it is not a simple application certificate import.

The author's parser format was inspected to read, without modifying, the current
host's ProductPolicy registry snapshot. The named CustomKernelSigners DWORD is
present with value zero. The separate exact `CodeIntegrity-AllowConfigurablePolicy`
entry was not found. An earlier substring search appeared to find both because
one name is a prefix of the other; only the bounded record parse distinguishes
the two. Evidence: `artifacts/diagnostics/win10-cks-policy-readonly.json`.

[Issue 8](https://github.com/HyperSine/Windows10-CustomKernelSigners/issues/8)
reports that the missing separate policy caused the old helper to fail repeatedly
in setup and produce a boot loop on Windows 10 2004. This is a concrete reason
not to run that helper unchanged here, not proof that every possible adaptation
is impossible. [Issue 18](https://github.com/HyperSine/Windows10-CustomKernelSigners/issues/18)
reports loss of the configuration on reboot in April 2026, without establishing
the reporter's exact Windows build.

A subsequent [source and read-only host study](windows10-cks-study.md) reproduced
the original policy lookup exception on this host and confirmed the missing
policy through the live native licensing query. The persistence worker depends
on that policy too. This is now a reproduced incompatibility in the unchanged
helper, not just an untested lead. Fixing the lookup would still leave firmware
trust and driver-load compatibility unproven. Evidence:
`artifacts/cks-study/result.json`. No activation or boot changes were performed.

### Windows Storage Spaces scripts

[Repository](https://github.com/freemansoft/win10-storage-spaces)

The scripts create a new tiered pool from raw/unpartitioned drives and allocate
its storage as a new volume. This is not an in-place filter for occupied D/M
volumes. They must not be run against the user's current data drives.

### WinSpd

[Repository](https://github.com/winfsp/winspd)

Provides user-mode implementations of virtual SCSI block devices. It is a
possible framework for a new cached disk, not an existing-volume interception
API. A proposal to place the existing D partition behind a second block device
would still need exclusive ownership and a solution for its active pagefile;
double-mounting the same writable filesystem is not a cache-coherence strategy.

### Dokan directory mounts: a narrower free alternative

The [upstream API](https://github.com/dokan-dev/dokany/blob/master/dokan/dokan.h)
supports mounting inside an empty NTFS directory, as well as a drive letter.
Two local directory-mode experiments served a new D fixture from an M-backed
SSD cache through the installed signed runtime without boot changes. See
[the proof and its limitations](dokan-directory-cache-proof.md). This refines the
earlier "virtual path" distinction: the mount may use a directory on D rather
than another letter. Preserving an existing folder path would still require an
unimplemented migration/recovery arrangement. This is not whole-D interception,
is read-only in the proof, and has no measured advantage over PrimoCache.
An unresolved S-mode process failure is retained in the test record.

## Reddit leads, not technical guarantees

- [Windows10: Use SSD as cache for HDD, January 2021](https://www.reddit.com/r/Windows10/comments/kz5qce/)
  discusses RST, Intel CAS, PrimoCache, and StoreMI. These are old individual
  experiences, not evidence of current hardware or licensing compatibility.
- [DataHoarder: preferably free Windows cache, April 2024](https://www.reddit.com/r/DataHoarder/comments/1c7w9ct/)
  contains a report of unsuccessful searching for a free alternative. This does
  not establish nonexistence.
- [PrimoCache alternative](https://www.reddit.com/r/chia/comments/q9wux1/primocache_alternative/)
  includes an explicit distinction between an ImDisk RAM disk and a disk cache.
  RAM disks do not meet the requested SSD-backed transparent block cache goal.

## Result

No reviewed repository establishes a ready, free, signed storage-cache backend
that meets all current host constraints. The unchanged Windows 10 CKS helper has
a reproduced host incompatibility, documented in the study above. Do not
describe it as tested, install it, change firmware trust, or claim D acceleration
on the basis of its README. The existing Super HDD v8 engine remains selected.
