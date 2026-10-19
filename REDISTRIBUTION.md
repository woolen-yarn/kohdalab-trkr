# Redistributing KohdaLab TRKR

Distribute the complete portable archive or installer with its `support/`
licenses, corresponding source archive, notices, and checksums. Do not ship
only the GUI executable. The repository MIT license applies to original
KohdaLab Rust code and the thin Python notebook bridge; dependencies and native
helpers retain their own licenses.

| Component | License boundary | Required material |
| --- | --- | --- |
| Rust GUI, engine, demo launcher, notebook bridge | MIT plus dependency licenses | Source and dependency notices included by the release packager |
| VISA helper | MIT | `native/visa_helper.cpp` and its notice |
| GPIB helper and its vendored components | GPL and upstream terms | Matching source, GPL text, and notices supplied with the packaged helper |

The Rust application invokes the GPIB helper as a separate process over its
documented standard-input/output protocol. This records an engineering boundary;
it is not a legal conclusion about GPL obligations. If the helper changes,
distribute the exact corresponding source and preserve its notices.

No vendor firmware, vendor VISA runtime, or manufacturer SDK is redistributed
by this project. Operators obtain their chosen VISA runtime and any adapter
firmware through sources that grant them permission. Do not imply vendor
endorsement.

The GPIB helper is excluded from the first-party SignPath allowlist in
[CODE_SIGNING_POLICY.md](CODE_SIGNING_POLICY.md). Redistribution permission and
eligibility for Foundation signing are separate questions.
