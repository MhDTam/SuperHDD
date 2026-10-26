# Reusing a signed backend — 2026-10-22

**Later user decision:** selected folders are acceptable and permanent free use
is mandatory. The current backend therefore uses installed signed Dokan binaries;
see [current scope](product-scope.md). The commercial review below is historical
and does not authorize a trial or purchase.

The target remains a 25 GB SSD cache on M for applications using the existing D
volume and paths. Host boot security and the D pagefile remain unchanged.

**User decision, October 22:** retain the PrimoCache-style storage-block-filter
architecture as the primary implementation. The SDK investigation below is a
recorded alternative, not an approved switch of backend. Continue the existing
v8 path for the requested whole-volume behavior.

## Finding

Obtaining our own kernel signature is a requirement of deploying our custom v8
driver on this host, not a requirement imposed on every possible implementation
of Super HDD. A reusable, already signed driver with an appropriate public API is
another architecture. Its signature does not authorize loading our custom SYS.

CBFS Filter is a concrete candidate: the vendor supplies a Microsoft-signed
filesystem minifilter and a Rust API. It intercepts requests to existing local
files, rather than requiring applications to use a new virtual drive. This is a
file-level alternative, not a drop-in storage-block-filter replacement.

## Verified API capabilities

The documented `on_before_read_file` callback permits the application to fill the
read buffer and complete the request without forwarding it. This requires
`ModifiableReadWriteBuffers`, a bounded copy, the returned byte count, and
`ProcessRequest=false`. The callback exposes offset, buffer capacity, IRP flags,
direction, file context, and handle context. The documentation explicitly
discusses cache-manager requests and `IRP_PAGING_IO`.

By default the SDK intercepts noncached requests, including transfers between
the Windows system cache and filesystem. Intercepting every RAM-cache hit is not
necessary to demonstrate SSD hits. Documentation alone does not establish safe
pagefile handling, measured speedup, or correct concurrent invalidation in our
application.

## Constraints before a prototype can activate

- The product has a fully functional 30-day trial. Continued use requires an
  appropriate commercial license; it is not a free permanent backend.
- Event handlers must use unbuffered I/O for every accessed file and follow the
  SDK's recursion/direct-file-operation rules. Buffered logging or cache-file
  access from callbacks can deadlock the system.
- First prototype: disposable, newly created test files; cache file on M outside
  the filtered source; source writes remain synchronous to the source, with
  cache invalidation before writes can make cached bytes stale.
- Whole-D filtering must wait for stable file identity/hard-link handling,
  truncation/delete/rename invalidation, mapped-write coherence, callback
  timeout/process-crash behavior, and bounded low-memory testing. Raw volume
  access and filesystem metadata are outside the proposed file cache's scope.
- Do not intercept `pagefile.sys` through a user-process dependency. Establish
  the SDK's exclusion behavior before attaching to a volume containing it.
- Benchmark active versus bypassed operation and verify bytes, actual SSD reads,
  and source reads. A signed dependency is not evidence of acceleration.

No SDK was installed, trial activated, license purchased, host driver enrolled,
or existing runtime replaced during this documentation review. Host D
acceleration remains inactive. The next bounded engineering step is a trial SDK
evaluation using owned test files, not preparation of an EV signing account.

## Primary sources

- [Product, signed-driver deployment, trial and licensing](https://www.callback.com/cbfsfilter)
- [Rust API: on_before_read_file](https://cdn.callback.com/help/CFM/rust/CBFilter.htm#CBFilter_e_BeforeReadFile)
- [Deadlock, recursion, and cached/noncached request rules](https://cdn.callback.com/help/CFM/rust/pages.htm)

The API pages retrieved during review identify build 9657, while the documentation
landing page identifies build 9743. Verify these contracts against the exact SDK
package selected for evaluation.
