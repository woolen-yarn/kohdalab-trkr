# API Cleanup Plan

Japanese version: [`api_cleanup_plan_ja.md`](api_cleanup_plan_ja.md).

This document tracks the cleanup work around the new `kohdalab.api` surface.
Most of the first refactor slice is now implemented; the remaining items are
maintenance and hardware-facing hardening.

## Current Stable Shape

Main entry point:

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

experiment = Experiment(load_config("config/kikuchi.json"))
plan = trkr_plan_from_config(experiment.config)
rows = experiment.run_trkr(plan=plan)
```

Important modules:

- `api/experiment.py`
  Public facade used by GUI, notebooks, CLI, and future apps.
- `api/session.py`
  Owns connected device handles and the auto-connect policy.
- `api/measurements.py`
  Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D workflows.
- `api/scan_plan.py`
  Plan objects and builders for corrected targets plus actual move points.
- `api/measurement_rows.py`
  Canonical row schemas, row constructors, and CSV output formatting.
- `api/status.py`
  Shared status strings and movement-status helpers for API callbacks and GUI
  worker signals.
- `api/config.py`
  Config normalization, defaults, output paths, and scan ranges.
- `api/device_requirements.py`
  Shared required/missing device rules.
- `api/scan_limits.py`
  Hardware-derived scan-limit hints.
- `api/notebook.py`
  Notebook display and plotting helpers.

## Completed Cleanup

### Row Schemas Are Centralized

Done:

- Added `api/measurement_rows.py`.
- Added canonical field constants:
  `MEASUREMENT_FIELDS`, `SIGNAL_MONITOR_FIELDS`, `TRKR_FIELDS`,
  `SRKR_FIELDS_BY_AXIS`, `STRKR_FIELDS`, and `SRKR_2D_FIELDS`.
- Added row constructors:
  `signal_monitor_row()`, `trkr_row()`, `srkr_row()`, and `scan2d_row()`.
- Added `fields_for_row()` / `fields_for_rows()` for CSV and snapshot order.
- Measurement writing and GUI Save Now use API field order.
- Tests assert exact row order.

Current schema:

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

`scan_axis` and generic `target` were replaced by `fast_axis`, `slow_axis`,
and axis-specific `target_*` columns.

### Scan Planning Is API-Owned

Done:

- Added `SignalMonitorPlan`, `TrkrPlan`, and `SrkrPlan`.
- Added `Scan2DPlan`, `StrkrPlan`, and `Srkr2DPlan`.
- Added explicit builders:
  `signal_monitor_plan()`, `trkr_plan()`, `srkr_plan()`, `strkr_plan()`,
  `srkr_2d_plan()`.
- Added config-derived builders:
  `signal_monitor_plan_from_config()`, `trkr_plan_from_config()`,
  `srkr_plan_from_config()`, `strkr_plan_from_config()`,
  `srkr_2d_plan_from_config()`.
- `Experiment.run_signal_monitor()`, `Experiment.run_trkr()`, and
  `Experiment.run_srkr()` accept `plan=...`.
- Added `Experiment.run_strkr()` and `Experiment.run_srkr_2d()` for
  non-serpentine 2D scans.
- `Scan2DPlan` exposes `fast_point_count`, `slow_point_count`, and
  `total_points`.
- Old `scan_points` / `target_points` arguments remain as compatibility paths.
- Plan length mismatches are guarded before a measurement starts.
- `apps/trkr_gui_measurement.py` is now only a compatibility re-export.

### Required Devices Are API-Owned

Done:

- Added `required_devices()` and `missing_devices()`.
- Added `Experiment.required_devices()` and `Experiment.missing_devices()`.
- Required devices respect config keys such as `lockin_key`,
  `delay_stage_key`, and SRKR `scanner_keys`.
- `trkr_gui.py` delegates Start checks to `Experiment.missing_devices(...)`.

Current defaults:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

### Auto-Connect Policy Is Explicit

Done:

- Added `DeviceSession(auto_connect=True)`.
- Added `Experiment(auto_connect=True)`.
- Read/move helpers respect `auto_connect`.
- Explicit `connect_device()` and `connect_all()` still connect when
  `auto_connect=False`.
- `trkr_gui.py` uses `Experiment(..., auto_connect=False)`.
- Tests cover default auto-connect, explicit-connect mode, and motion methods
  with auto-connect disabled.

### Session Ownership Is Tested

Done:

- All `Experiment.run_*()` methods pass the existing `Experiment.session` into
  the measurement layer.
- Caller-supplied sessions are not disconnected by measurement functions.
- Temporary standalone measurement sessions disconnect on error.
- GUI worker tests verify workers receive an `Experiment` object and call
  `Experiment.run_*()` rather than constructing an experiment from config.

### GUI Helpers Were Split Out

Done:

- Config parsing/saving helpers live in `apps/trkr_gui_config.py`.
- Coordinate label/unit helpers live in `apps/trkr_gui_coordinates.py`.
- Device reference helpers live in `apps/trkr_gui_devices.py`.
- Output path helpers live in `apps/trkr_gui_output.py`.
- Signal display helpers live in `apps/trkr_gui_signal.py`.
- Snapshot formatting lives in `apps/trkr_gui_snapshot.py`.
- Plot data helpers live in `apps/trkr_gui_plot.py`.
- Measurement plan imports in `apps/trkr_gui_measurement.py` re-export the API
  builders for compatibility.

### GUI Hardware Access Is Worker-Routed

Done:

- Added GUI workers for live status, device commands, manual moves, and
  measurement runs.
- Connect, Disconnect, Initialize, `Use TC*4`, manual Move, and live status
  reads no longer run directly on the Qt main thread.
- VISA resource and COM-port refreshes also run through a GUI worker.
- `DeviceCommandWorker` is now a long-lived GUI command actor instead of a
  per-command worker.
- Window close routes `disconnect_all()` through the command actor.
- Manual delay-stage/scanner moves forward progress through worker signals.
- During a manual move, the moving device is polled only by `MoveWorker`; the
  live worker reads only unrelated lock-in status.
- GUI worker tests cover live status, device commands, manual move progress,
  and measurement worker routing.

### Lock-In Setting Writes Are API-Level

Done:

- Added `api.devices.lockin.set_lockin_settings()`.
- Added `DeviceSession.set_lockin_settings()`.
- Added `Experiment.set_lockin_settings()`.
- Exported `set_lockin_settings` from `kohdalab.api`.
- Setting writes follow the same `auto_connect` policy as lock-in reads.
- Everyday GUI currently keeps lock-in settings read-only; the API path remains
  available for scripts or a future operator workflow.
- Tests cover public helper behavior, no-op behavior, explicit-connect mode,
  default auto-connect mode, and the `Experiment` facade.

### LiveStatus Carries Lock-In Overload

Done:

- `api.models.LiveStatus` now includes `lockin_overload`.
- `DeviceSession.read_live_status()` reads signal, settings, and overload for
  the active connected lock-in.
- GUI full live-status refreshes no longer need a separate overload read.

### DeviceSession Serializes Per-Device I/O

Done:

- Added per-device reentrant I/O locks inside `DeviceSession`.
- Lock-in signal/settings/overload reads, wait-time reads, and setting writes
  now serialize against the same connected lock-in handle.
- Motion-device moves hold only that motion device's lock, so lock-in live
  status can continue while a delay stage or scanner move is running.
- GUI `Use TC*4` ignores rapid repeat clicks while another device command is
  active and reports command failures as generic Device Errors instead of
  Initialize Errors.

### Measurement Status API Is Centralized

Done:

- Added `api/status.py`.
- Exported status constants such as `STATUS_RUNNING`, `STATUS_WAITING`,
  `STATUS_READING_LOCKIN`, `STATUS_SLOW_AXIS_READY`, and `STATUS_STOPPED`.
- Exported movement helpers:
  `moving_scanner_status()`, `moving_axis_status()`, and
  `moving_axis_from_status()`.
- `DeviceSession.move_delay_stage()` and `DeviceSession.move_scanner()` emit
  shared movement status strings.
- Measurement workers and GUI live position labels consume the same API status
  vocabulary for manual moves and measurement-owned moves.

### Delay-Stage Motion Is Hardened Against Repeat Clicks

Done:

- Added a short manual-move cooldown in the GUI at move start and completion.
- Delay-stage wrappers now cache microstep division after the first successful
  controller query.
- SHOT-302GS/GSC01 microstep parsing retries transient empty/corrupted
  responses before failing.
- GUI scan-limit hints use cached microstep values only, avoiding direct
  controller `?S`/`?MS` queries from the Qt main thread during motion.

### Interface Cache Behavior Is Tested

Done:

- Added hardware-free tests for `interfaces.scanner` connection caches.
- Same scanner config reuses an existing connected handle.
- CONEXAGAP U/V axes on the same COM port share one serial handle.
- Disconnecting one CONEXAGAP axis keeps the shared serial open while another
  axis still uses it.
- Disconnecting the last shared axis closes the serial and removes it from the
  serial cache.
- Disconnect-all closes each serial exactly once.
- CONEXCC scanners on different COM ports use different handles and serials.
- Added hardware-free tests for `interfaces.lockin` and
  `interfaces.delay_stage` connection caches.
- Lock-in and delay-stage tests cover same-config reuse, stale-handle
  replacement, cache key separation, targeted disconnect, and disconnect-all.

### Scanner Coordinate Naming Is Normalized

Done:

- Added scanner-specific coordinate normalization.
- SRKR plans and measurement rows now prefer `interface` for scanner actuator
  mm/deg coordinates.
- `instrument` and `device` remain accepted scanner compatibility aliases and
  normalize to `interface`.
- Direct scanner moves also normalize `um`/`sample_um` to `measurement` and
  `mm`/`deg`/`pos_*` aliases to `interface`.
- Tests cover scanner coordinate normalization and direct scanner move aliases.

### Everyday GUI Is The Only Operator Entry Point

Done:

- Kept `trkr_gui.py` as the everyday measurement-unit GUI.
- Kept `trkr_gui_advanced.py` only as a reference copy of the previous
  raw/interface coordinate GUI.
- Removed the advanced GUI script entry point from the current operator
  workflow.
- Added a small test so `kohdalab-gui` stays declared and
  `kohdalab-gui-advanced` stays unexposed in `pyproject.toml`.

### Hardware Smoke Test Checklist Is Documented

Done:

- Added `docs/hardware_smoke_test.md`.
- The checklist covers preflight, connect/disconnect, initialization, small
  manual moves, short Signal Monitor/TRKR/SRKR/STRKR/SRKR 2D runs, CSV
  headers, lock-in wait-time recovery, and clean restart.
- `docs/trkr_gui.md` links to the checklist for post-change GUI verification.

## Current Design Rules

1. `Experiment` owns the connected `DeviceSession`.
2. Everyday GUI measurements must use already connected handles.
3. Standalone measurement functions may create temporary sessions only when no
   session is supplied.
4. Supplied sessions must never be disconnected by measurement functions.
5. Public row schemas are explicit and stable.
6. GUI layout/state helpers stay in `apps/`.
7. Device protocol quirks stay in `instruments/`.
8. Unit normalization and hardware-handle reuse stay in `interfaces/`.
9. GUI hardware I/O should be worker-routed; the main Qt thread should update
   widgets, build runtime config, and handle signals.
10. `DeviceSession` should serialize hardware I/O at the narrowest practical
    device-resource scope so same-device commands cannot overlap, while
    unrelated live status can still continue.

## Remaining Cleanup

### Hardware Smoke Tests

Static and hardware-free tests are useful, but the risky surface is hardware.

Manual checks still needed using `docs/hardware_smoke_test.md`:

- everyday GUI smoke test against real hardware
- updated notebooks against connected instruments

## Recommended Next Slice

Best next documentation/software slice:

1. Use `docs/hardware_smoke_test.md` during the next lab run.
2. Update notebooks and docs from any hardware-specific findings.

Best next lab slice:

1. Run the everyday GUI visibly.
2. Connect each device individually and with Connect All.
3. Run one short Signal Monitor, one short TRKR, one x/y SRKR pair, one STRKR,
   and one SRKR 2D.
4. Save rows and confirm the CSV headers match `api.measurement_rows`.
