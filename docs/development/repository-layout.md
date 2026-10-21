# Repository layout

KohdaLab IV, TRKR, and Camera use the same organizing principles.

| Location | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | One authoritative Rust workspace and dependency lock at the root |
| `crates/<package>/` | Rust packages; implementation and package-owned resources stay together |
| `bindings/`, `notebook/` | Optional scripting/Notebook interfaces, when the product provides them |
| `native/` | Native helper sources and upstream licensing, when required |
| `packaging/` | Static installer/driver-setup inputs, when required |
| `scripts/` | Build, package, source-identity, and validation entry points |
| `tests/` | Cross-package packaging/compatibility checks, when applicable |
| `docs/` | User guides, development notes, references, validation evidence, and images |
| `target/`, `build/`, `dist/` | Ignored local build outputs; never source inputs |

Optional directories exist only when they have a real role. Native backends and
public package/bin names are product-specific; this layout does not introduce a
shared runtime dependency between repositories. Cargo commands run from the root.
The former `next/` preview workspace is no longer the maintained source location.
Existing installed application settings and distribution layouts are unchanged.

## Documentation rules

The root README is a short product page: overview, interface, downloads, supported
hardware, quick start, development, and documentation links, in that order.
Detailed procedures belong in `guides/`; contributor architecture and build
maintenance in `development/`; stable protocol/format/support descriptions in
`reference/`; dated/versioned measurements in `validation/`; figures in `images/`.
`docs/README.md` is the navigation index. Do not copy the full manual back into
the landing page. Preserve hardware qualification limits and historical evidence.

## Changes to paths

Move a file and update its consumers together: Cargo/build scripts, helper lookup,
CI path filters, packaging source allowlists, client discovery, examples, and
documentation links. Keep installed resource paths and public executable names
stable. Compare package metadata and dependency locks, run device-free checks,
and inspect source archives so ignored outputs or private data cannot leak.
