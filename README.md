<img src="crates/trkr-desktop/assets/icon.png" width="64" height="64" align="right" alt="KohdaLab TRKR icon">

# KohdaLab TRKR

[![CI](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml/badge.svg)](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml)
[![Release](https://img.shields.io/github/v/release/woolen-yarn/kohdalab-trkr)](https://github.com/woolen-yarn/kohdalab-trkr/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Rust desktop application for time-resolved Kerr rotation measurements, live lock-in monitoring, and stage/scanner control.

[Download](https://github.com/woolen-yarn/kohdalab-trkr/releases) · [Documentation](docs/README.md) · [Report an issue](https://github.com/woolen-yarn/kohdalab-trkr/issues)

- Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D.
- Live XY/Rθ traces, retained spatial scans, rotated u/v axes, and heatmaps.
- Shared Rust measurement engine for the desktop GUI and optional Python/Jupyter client.

## Interface

![KohdaLab TRKR interface](docs/images/gui-trkr.png)

Synthetic TRKR data; no instruments connected. The same interface is used on Windows and macOS.

## Downloads

| Channel | Version | Platforms |
| --- | --- | --- |
| Stable | [0.3.4](https://github.com/woolen-yarn/kohdalab-trkr/releases/tag/v0.3.4) | Windows x64 Setup / portable ZIP; macOS Apple silicon DMG / ZIP |

Rust and Python are not required to run the packaged application. Current Windows
artifacts are unsigned; download packages and checksums together from Releases.
macOS bundles use ad hoc signing and are not Apple-notarized.

## Supported hardware

| Role | Implemented models |
| --- | --- |
| Lock-in | SR7265, SR830, SR5210, LI5640 |
| Delay controller | SHOT302GS, GSC01, GSC01A |
| Scanner controller | CONEX-CC, CONEX-AGAP |

See [hardware and transport](docs/reference/hardware.md) for stage/actuator presets, GPIB adapters, and validation scope. Implemented protocol support does not imply physical qualification of every device/firmware combination.

## Quick start

1. Install the application or extract the complete portable package.
2. Explore synthetic data using the separate Demo application before connecting hardware.
3. Load a reviewed profile, check the selected devices, ports, origins, calibration, and limits, then connect deliberately.
4. Choose a workflow and output location. Keep the CSV and its `.run.json` metadata together.

Python/Jupyter access: [client guide](bindings/python/README.md) · [notebooks](notebook/).

## Development

Run commands from the repository root. The root `Cargo.toml` and `Cargo.lock`
are the workspace entry point; product packages live under `crates/`.

```sh
cargo test --workspace --locked
cargo build --release --locked
cargo run --locked --bin kohdalab-trkr
```

```text
crates/       Rust application, measurement core, motion, and transport
bindings/     Thin Python/Jupyter client
native/       Isolated native helpers and upstream notices
notebook/     Rust-backed notebook examples
scripts/      Build, package, and validation entry points
tests/        Packaging/client tests and frozen reference fixtures
docs/         Guides, development notes, references, validation, images
```

See [repository layout](docs/development/repository-layout.md) and
[CONTRIBUTING.md](CONTRIBUTING.md) before changing device or build boundaries.
Tests must not connect hardware, move stages, enable outputs, or alter drivers.

## Documentation

- [Documentation index](docs/README.md): the entry point for all guides and references.
- `docs/guides/`: installation and everyday operation.
- `docs/development/`: architecture, extension points, and build/release maintenance.
- `docs/reference/`: hardware, protocols, formats, and compatibility contracts.
- `docs/validation/`: versioned evidence and measured limitations.
- `docs/images/`: repository screenshots and figures.

## KohdaLab applications

[IV](https://github.com/woolen-yarn/kohdalab-iv) · [TRKR](https://github.com/woolen-yarn/kohdalab-trkr) · [Camera](https://github.com/woolen-yarn/kohdalab-camera)

The applications share a root Cargo workspace, `crates/` source packages, focused
build scripts, and the same documentation categories. Hardware backends and
qualification remain product-specific.

## License

Project code is [MIT licensed](LICENSE). Native helpers and redistributed
dependencies retain their own licenses; keep the accompanying notices and source.
See [redistribution terms](REDISTRIBUTION.md), [code signing](CODE_SIGNING_POLICY.md), and [privacy](PRIVACY.md).
