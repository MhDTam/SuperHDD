# Paging-capable storage filter v8

The v8 candidate extends the mounted-source storage filter to buffered and mapped
reads that arrive as paging I/O. The user interaction remains source D, SSD M,
and a capacity budget; there is no page-file relocation or physical repartition.

## Implementation

- A separate version-8 endpoint and policy prevent an older client from silently
  treating the new paging contract as the v7 passthrough implementation.
- The cache storage stack must accept a paging-path device-usage notification
  during setup. Rejection fails attachment. The dependency is balanced on close.
- The dedicated reader reserves its aligned 64 KiB scratch buffer, IRP and MDL
  storage before activation. Ordinary and paging reads use these resources;
  mapping failures, full queues and unsupported reads fall back to the source.
- SHA-256 uses the Microsoft CNG provider opened with `BCRYPT_PROV_DISPATCH` and
  a preallocated reusable hash object. Known-vector and reset checks run before
  activation. Digest failure disables trust and falls back to source; it never
  authorizes a cache hit. The persisted digest format and full-block verification
  remain unchanged. Slot/index metadata and scratch space still consume RAM;
  there is no dedicated RAM data-cache tier.
- Writes retain source write-through semantics and invalidate overlapping cached
  blocks. Dump and hibernation roles remain excluded from caching.
- The service, installer helper and desktop use independently pinned v8 paths and
  qualification hashes. Existing active v7/v6 allocations retain their controls.
  Development builds have no qualified hashes and reject host activation.

## Evidence and limits

The pre-CNG kernel run `c2a718909543441c9c94494f8299e4eb` demonstrated actual
buffered and mapped paging SSD hits with correct bytes at 43–116 MiB available
guest RAM. The new guest source was a file on the host HDD D; the guest cache was
in new files on host SSD M. All temporary parent files were removed and the VM
ended off with zero assigned memory. This did not activate a host filter on D.
Some mapped reads were slower than the paused baseline; these observations
motivated the CNG implementation and are preserved, not presented as a speedup.

The first CNG SCM attempt `c671372fd8484fadb59833b5754fe440` stopped because its
test client expected native version 7. Its protected status showed an active v8
service under LocalSystem. The client check was corrected. Fresh run
`405300c0a94349fa807c3bb2c732a625` completed successfully on 2026-10-22: all 13
SCM checks, the full application workload, restoration, cleanup after cold boot
and final VM shutdown passed. The exact tested service and physical/VM driver
hashes are bound in `artifacts/paging-runtime-candidate/manifest.json`.

The CNG workload used three fresh 16 MiB file pairs per access mode, with the SSD
cache warmed and 49–78 MiB physical guest memory available during the measured
reads. All six active samples were faster than their paused pairs; every pair
observed actual storage I/O. These are bounded sample results, not application
launch-time guarantees:

| Access mode | Paused median | Active median | Ratio of medians |
| --- | ---: | ---: | ---: |
| Buffered random reads | 449.071 ms | 222.231 ms | 2.02 |
| Memory-mapped reads | 658.238 ms | 306.040 ms | 2.15 |

All byte oracles passed with no new cache errors. Persisted-record revalidation
and real automatic SCM restart after killing the owner also passed. Current Rust
workspace checks passed (121 tests), UI checks passed (23), Clippy passed, and
both physical/VM kernel builds passed static analysis. The desktop and service
were built and staged with verified hashes. The final host read-only check
confirmed D healthy/online, its 14,848 MiB page file unchanged, Secure Boot still
enabled, test signing disabled and no active Super HDD host driver. The VM ended
off with zero assigned RAM. Temporary HDD and cache parent files were removed.

Current reports are `artifacts/paging-session.json` and
`artifacts/runtime/kernel-lab/paging-service.json`. The workload compares the first
buffered/mapped read of unique files with caching paused and active, alternates
ordering, checks bytes, and records paging hits, SSD bytes, source worker reads,
bypasses, cache errors and actual memory pressure. RAM-only reads do not establish
SSD acceleration. Small VM samples do not predict every application or constitute
physical Windows qualification. The production image remains unsigned.

## Reproduce

Build with `scripts/build-paging-driver.ps1` and `-VmTest` for the guest-only
source policy; use `-Analyze` for static analysis. `test-paging-service.ps1
-HostHddFiles` runs the bounded SCM and workload suite in the owned networkless
guest, restores its paging policy, removes run-owned data and verifies cleanup
after cold boot. `stage-paging-runtime.ps1` requires current complete proofs and
only creates a workspace review artifact.

Microsoft documentation:
[nonpageable storage paths](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/restrictions-on-pageable-code-in-storage-drivers),
[nonpaged CNG provider](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptopenalgorithmprovider),
[reusable hash object](https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptcreatehash).
