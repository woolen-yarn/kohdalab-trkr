# Roadmap

This roadmap tracks work that keeps KohdaLab TRKR reliable for real experiments while preserving a testable Python core.

## v0.1.0 - Project Baseline

- MIT license, contribution guide, security policy, safety notes, and changelog.
- GitHub issue templates, pull request template, CODEOWNERS, Dependabot, and CI on Ubuntu and Windows.
- Windows instrument-PC setup documentation.
- Repository topics, labels, branch protection, and release metadata.

## v0.2.0 - Measurement Reliability

- Broaden regression tests around measurement sequence generation and worker cancellation.
- Add validation checks for config ranges, instrument resources, timing parameters, and output paths.
- Preserve more run metadata with measurement outputs for later analysis and reproducibility.

## v0.2.1 - Reliability Hardening

- Enforce complete statement and branch coverage across supported Python and operating-system combinations.
- Harden device-session ownership, measurement cleanup, atomic output recovery, and controller validation.
- Verify locked dependencies and reproducible wheel/source distributions in CI.

## v0.3.0 - Hardware Abstraction

- Clarify stable public driver/session APIs for supported instruments.
- Add simulated hardware sessions for GUI and notebook development without connected instruments.
- Document validated wiring, resource names, and instrument settings for common TRKR setups.

## v0.4.0 - Workflow Polish

- Improve GUI diagnostics for disconnected devices, stale readings, and long-running scans.
- Add notebook examples for loading, plotting, and comparing TRKR/SRKR/STRKR datasets.
- Add release notes that highlight measurement-facing behavior changes.

## Later

- Consider remote execution support for Windows instrument PCs.
- Consider richer metadata exports for analysis pipelines.
- Consider optional hardware-in-the-loop smoke tests for lab-maintained systems.
