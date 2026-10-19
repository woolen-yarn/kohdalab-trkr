# Changelog

## 0.3.1 — 2026-10-19

- Automatically discover installed Windows/macOS applications from the Notebook client.
- Share executable/profile discovery across six notebooks; prefer GUI-saved profiles over source examples.
- Add `prepare_profile()` and path-free `Client()`, with UTF-8 native communication.
- Prefer the bundled client over an older imported client in portable/installed notebooks.
- Include the updated notebooks, client and migration guide in Setup, portable ZIP and Mac distributions.

## 0.3.0 — 2026-10-19

- Replace the Python GUI, drivers and measurement engine with the shared Rust application.
- Windows portable and per-user Setup with desktop shortcut; macOS native app and DMG.
- Preserve Signal Monitor, TRKR, SRKR, STRKR and SRKR 2D, JSON profiles and CSV schema.
- Live XY/Rθ monitor, TRKR log display, Python-compatible heatmaps and paired u/v validation.
- Device-local progress status, acquired/total Step, fixed sidebars and compact dark interface.
- Read-only Rust CSV demo and six Rust-backed Notebook examples.
- Source-built release CI, checksums, origin metadata and SignPath application preparation.

Earlier Python versions are preserved in Git tags and GitHub Releases.
