# Common control API validation

Date: 2026-10-29. Target: 0.4.4. Host: macOS ARM64.

- Device-free Rust tests: 274 passed across the selected application/workspace and common contract suites.
- Shared Python SDK: 9 tests passed, including authenticated loopback errors, exact job links, ambiguous timeout boundaries and pending cleanup.
- Actual Demo GUI: 360 synthetic points through shared SDK start/result/wait/release. Common connect rejects hardware connection in Demo, as intended.
- Physical instruments were not opened or qualified for this change. Windows packaging/CI evidence belongs to the matching PR/release; these local tests are not a Windows hardware pass.

The existing native API, scientific storage and product-specific safety policy remain separate from the additive common namespace. See [the common contract](../reference/common-control-api.md).
