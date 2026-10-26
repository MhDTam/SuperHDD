# Super HDD: request for EV eligibility and Windows driver signing

Prepared 2026-10-19. This is an unsent inquiry, not an order or authorization to
publish the source, accept agreements, provide identity documents or buy anything.

**Superseded by the user's free-only budget constraint.** The paid route below is
retained as historical research, not the selected solution. Current findings and
the unsent OSS sponsorship eligibility inquiry are in
[free-signing-research.md](free-signing-research.md). No free Microsoft kernel-driver
signing route has been confirmed.

The developer has no Hardware Developer account or EV certificate. The target is
our own Windows 10/11 x64 storage-filter driver. GitHub provenance and ordinary
application signing are not substitutes for Microsoft kernel-driver signing.

## A concrete individual-publisher route to check

SSL.com advertises [Sole Proprietor EV](https://www.ssl.com/products/software-integrity/code-signing/ev-sole-proprietor/)
for independent publishers, including driver submission. Its displayed one-year
certificate price on the date above was USD 359; key storage/cloud signing and
taxes may cost extra. Its page says an incorporated business is not required.
This does **not** confirm issuance to an Iraqi resident or acceptance of this
specific publisher's registration. Obtain both confirmations before purchasing.
Do not buy an IV/OV application certificate as a substitute.

Microsoft's [registration instructions](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/hardware-program-register)
require identity/account setup, an associated EV certificate and an authorized
contact to accept the agreements. Microsoft explains the separate
[signing requirements](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs)
and [HLK/attestation signing scopes](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings).
An EV purchase alone does not produce a Microsoft-signed production driver.

## Ready-to-send inquiry

Subject: Individual publisher in Iraq — EV certificate and Microsoft kernel-driver submission

I am an independent developer based in Iraq working on Super HDD, a Windows x64
storage-filter driver and desktop application. I do not currently have an EV
certificate or a Microsoft Hardware Developer account.

Before I place an order, please confirm:

1. Can you validate and issue Sole Proprietor EV to an Iraqi resident? What
   identity/address documents are required, and must any business be registered?
2. Can that certificate be associated with Microsoft's Hardware Developer program
   for this individual publisher? Please distinguish certificate issuance from
   Microsoft's separate account-verification requirements.
3. Which cloud/HSM option is valid for driver-submission CAB signing? Please
   provide the complete first-year and renewal cost, including storage/signing,
   hardware, delivery and any other mandatory fees.
4. If issuance or Microsoft account registration cannot complete, what refund
   policy applies?

The driver is still under development. It has passed scoped signed-VM data and
stress tests; physical deployment, production qualification and signed installation
are incomplete. No immediate production or certification claim is made.

## Engineering work that remains ours

- Finish and validate physical-source eligibility, backing dependency/recursion
  checks and the owned SSD-container lifecycle.
- Integrate the privileged controller and exact-source filter deployment, including
  rollback and recovery, with the desktop.
- Complete relevant reliability, Windows-version and hardware qualification.
- Submit the exact qualified package through the verified publisher's account,
  verify the returned signature under Windows kernel policy, then test that exact
  package before host activation.

No vendor cache backend, host test-signing, certificate-trust change or security
exception is used by this route.
