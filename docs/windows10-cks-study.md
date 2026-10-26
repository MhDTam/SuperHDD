# Windows 10 Custom Kernel Signers: host compatibility study

Date: 2026-10-22. Target: Windows 10 Pro x64 build 19045, Secure Boot enabled,
TestSigning disabled. Requested outcome: Super HDD v8 transparently caches
existing D paths using 25 GB on M, without changing host boot security.

## Decision

Do not run the upstream activation helper on this host. Its policy lookup fails
against the current host data, and that failure occurs after persistent setup
changes in the full activation sequence. This conclusion is supported by a
read-only reproduction, not just by an old issue report.

This does not establish that every possible Windows 10 CKS adaptation is
impossible. It does establish that this repository is not a working solution
under the current constraints. No host cache was activated by this study.

## Source and reproducible evidence

Repository: [HyperSine/Windows10-CustomKernelSigners](https://github.com/HyperSine/Windows10-CustomKernelSigners).
Reviewed commit: `d3b93a4fabfd3800fe91b7298a2cabb4efd880b7`, dated
2020-01-22. The fetched master commit resolved to this version during the study.

- Source provenance and SHA-256 hashes: `artifacts/cks-study/provenance.json`.
- Read-only harness: `tools/cks-study/readonly-study.cpp`.
- Build, import audit and execution: `tools/cks-study/run-readonly-study.ps1`.
- Actual results: `artifacts/cks-study/result.json`.
- Build warnings/log and imports: `artifacts/cks-study/build.log`, `imports.txt`.

The harness compiles only the unchanged upstream policy model and parser, plus
our separate entry point. It never compiles or invokes the upstream setup entry
point or kernel driver. Registry access requests only `KEY_QUERY_VALUE`. It also
queries the live licensing table through `NtQueryLicenseValue`, a read operation.
The complete licensing blob is neither printed nor saved. Record boundaries are
validated before passing the in-memory copy to the upstream parser.

Observed results, using MSVC 14.44 on this host:

| Exact policy | Registry | Upstream lookup | Live license query |
| --- | --- | --- | --- |
| `CodeIntegrity-AllowConfigurablePolicy` | Absent | Throws `out_of_range` | `0xC0000034` (name not found) |
| `CodeIntegrity-AllowConfigurablePolicy-CustomKernelSigners` | DWORD 0 | Exact match | Success, DWORD 0 |

Both const and mutable upstream lookups produced the same outcomes. The policy
blob and Windows Setup `CmdLine`/`SetupType` values compared equal before/after.
No activation helper, driver load, reboot, firmware operation, or policy write
was executed. Import inspection found no listed registry-write, shutdown,
privilege-adjustment, driver-load or licensing-update APIs in the harness.

## Failure mechanism

In the upstream [setup helper](https://github.com/HyperSine/Windows10-CustomKernelSigners/blob/d3b93a4fabfd3800fe91b7298a2cabb4efd880b7/EnableCustomKernelSigners/_tmain.cpp),
the normal entry sets `SYSTEM\Setup\CmdLine` and `SetupType` before asking about
reboot. On the setup entry, the missing policy is accessed outside the parser's
exception handler. Cleanup of `CmdLine` is reached only after successful policy
processing. There is no surrounding catch that restores the prior setup values.
Our harness catches the reproduced exception; the original entry point does not.

[Issue 8](https://github.com/HyperSine/Windows10-CustomKernelSigners/issues/8)
reports this combination causing repeated setup failures on Windows 10 2004.
The reporter also states that adding the missing entry stopped the loop but did
not make the driver load. That is a reporter's observation on another build,
not a host driver-load test here. We did not run the full helper to reproduce a
boot loop.

## Why a lookup guard is insufficient

The [persistence worker](https://github.com/HyperSine/Windows10-CustomKernelSigners/blob/d3b93a4fabfd3800fe91b7298a2cabb4efd880b7/CustomKernelSignersPersistent/cksp_main.c)
first queries the base policy and follows its error path if that query fails.
The native host query above independently reproduced the missing-name condition.
The worker's updater edits policies already present in a blob; it does not add
the absent entry. It invokes `ExUpdateLicenseData` to maintain the alteration
when Windows licensing data changes. This is a continuing dependency on internal
licensing behavior, not a one-time certificate-store import. Driver runtime
behavior was assessed from source only; the driver was not built or loaded.

Separately, the [project's prerequisites](https://github.com/HyperSine/Windows10-CustomKernelSigners/blob/d3b93a4fabfd3800fe91b7298a2cabb4efd880b7/README.md)
require control of the UEFI Platform Key and a signed boot policy. Fixing a C++
lookup cannot supply that trust chain. Secure Boot being enabled does not mean
firmware trust remains unchanged. This requirement conflicts with the user's
current host constraints.

An additional isolated fixture demonstrated a search defect: for valid sorted
records A/B/C, the original lookup fails to find existing A, while B and C work.
This is consistent with the early break before comparing the lower endpoint in
[ProductPolicy.cpp](https://github.com/HyperSine/Windows10-CustomKernelSigners/blob/d3b93a4fabfd3800fe91b7298a2cabb4efd880b7/EnableCustomKernelSigners/ProductPolicy.cpp).
The separate use of exact +/-1 comparison results is a portability concern; a
wrong-name lookup was not observed in this run.

## What remains unproven

- Loading a custom-signed kernel driver using this method on Windows 10 19045.
- Persistence across licensing refreshes, reboot and servicing on that build.
- A reliable rollback for setup, firmware trust and boot policy changes.
- Host physical-disk cache qualification, independent of signer policy.

If this research is extended, the appropriate experiment is a disposable UEFI
Windows 10 19045 VM with controlled firmware keys and recovery snapshots. The
existing Generation 1 Server 2022 lab does not exercise the same trust path.
Creating that environment or changing this host was not part of this study.
Microsoft's newer documented Windows 11 CKS deployment is a separate route;
its requirements must not be inferred from this older helper.
