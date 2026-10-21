# Changelog

## 0.3.4 — 2026-10-21

- Make the fake-device homing progress test tolerate an additional time-driven position read on slow CI runners, and cover a delayed-ready response without changing production motion code.

- Preserve existing measurement, GUI, JSON, CSV and Notebook behavior while separating recording, sequencing, profile compatibility and tests into focused modules.
- Share workflow names, ordering, axis rules, output names and plot counts between core validation and the GUI.
- Centralize scanner capabilities, actuator defaults, lock-in setting options and auto commands; use one compensation plan for preflight and execution.
- Represent worker phases and fixed control/transport failures with internal types while preserving public strings and error precedence.
- Freeze unversioned profile/API/recording revision-1 contracts with additional fake-device and compatibility tests; document extension points.
- Add native demo GUI screenshots and supported instrument, stage/actuator preset and transport tables to the README. Keep documentation captures independent of operator profiles and paths.

## 0.3.3 — 2026-10-21

- Make manual-position polling verification deterministic under slow CI scheduling, including a delayed-ready case.
- Include the motion/live-display/configuration fixes listed below. Version 0.3.2 did not publish binaries because its timing-dependent test failed on CI.

## 0.3.2 — 2026-10-21

- Audit all 50 legacy runtime source files and index 219 legacy test functions; document remaining GUI, profile, plotting, maintenance and Notebook API gaps.
- Preserve singular `measurement` profile ranges/zeros and validate manual-move device references before connection.
- Enable configured spatial compensation for GUI/Notebook standalone Move and the post-initialization instrument-zero move; preflight the entire approach before positioning or homing.
- Restore return-to-zero hysteresis for enabled CONEX-CC scanners; use the common scan/return distance (100 µm default), preflight limits and preserve Stop cancellation.
- Compensate each 2D spatial fast-axis line before acquisition; omit redundant uncompensated line-start visits and never return to experiment zero between lines.
- Restore Cartesian 2D slow-before-fast motion, fast-before-slow completion returns and measurement-zero returns after raw-coordinate scans.
- Restore GUI initialization's calibrated instrument-zero move, with explicit action wording and opt-in native command compatibility.
- Accept legacy coordinate/output aliases; reject unsupported named-instrument selection before connecting instead of silently selecting a different device.
- Preserve legacy hardware aliases, device omissions and calibration values through GUI profile load/save; canonicalize known output aliases while retaining unknown metadata.
- Match frozen Python CSV common columns and numeric text; retain all scan timing in the additive `acquisition_elapsed_s` column.

- Retain the latest plot per SRKR axis and ordered STRKR/SRKR 2D axis pair; restore original calibration, progress and recorded CSV when switching back.

- Keep the XY/Rθ live monitor updating at approximately 100 ms during motion and scan settling, independently of recorded points.

- Show device-local Moving status and actual encoder positions during manual moves (approximately 100 ms updates).
- Make Initialize explicitly confirm mechanical homing for delay stages/CONEX-CC; CONEX-AGAP enables closed-loop operation.
- Report initialization completion, refresh delay calibration, and preserve cancellation/STOP on motion errors.

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
