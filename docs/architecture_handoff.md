# KohdaLab Architecture Handoff

Japanese version: [`architecture_handoff_ja.md`](architecture_handoff_ja.md).

This note describes the current working-tree architecture so future changes can
continue without reconstructing the refactor history.

## Goal

Notebooks, the desktop GUI, the CLI, and future apps should control the lab
through one public API.

Dependency direction:

```text
apps / notebooks / CLI / future web UI
  -> kohdalab.api
  -> kohdalab.interfaces
  -> kohdalab.instruments
```

`kohdalab.api` is the public workflow layer. The old `services` and
`measurement` packages have been removed from this working tree.

## Current Public API

The central object is `Experiment`.

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

config = load_config("config/kikuchi.json")
experiment = Experiment(config)
experiment.connect_all()
status = experiment.read_live_status()
experiment.move_delay_stage(0.0, coordinate="measurement")
plan = trkr_plan_from_config(config)
rows = experiment.run_trkr(plan=plan)
experiment.disconnect_all()
```

`Experiment` owns one `DeviceSession`. `Experiment.run_*()` always passes that
session into the measurement layer and leaves it connected after the run.
Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D accept their corresponding plan
objects.
`LiveStatus` includes connected-device state, position, lock-in signal,
lock-in settings, and lock-in overload.

Standalone measurement functions in `api.measurements` still work for simple
scripts; when no session is supplied they create a temporary session and
disconnect it at the end.

## Important Modules

- `src/kohdalab/api/config.py`
  Config load/save, normalization, validation, output paths, scan ranges, and
  instrument key lookup.
- `src/kohdalab/api/session.py`
  `DeviceSession`, connected handle ownership, auto-connect policy,
  initialization, live status, move operations, and device reference aliases.
- `src/kohdalab/api/experiment.py`
  Public facade for notebooks, GUI, CLI, and future apps.
- `src/kohdalab/api/measurements.py`
  Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D row-producing workflows.
- `src/kohdalab/api/scan_plan.py`
  `SignalMonitorPlan`, `TrkrPlan`, `SrkrPlan`, `StrkrPlan`, `Srkr2DPlan`, and
  config-derived builders. TRKR/SRKR plans pair corrected display targets with
  actual absolute move points. STRKR/SRKR 2D plans define fast/slow corrected
  axes and expose point counts for progress/ETA.
- `src/kohdalab/api/measurement_rows.py`
  Canonical row field order, row constructors, CSV output formatting, and row
  field inference.
- `src/kohdalab/api/status.py`
  Shared measurement/motion status strings and helpers that map movement status
  back to `t`, `x`, or `y`.
- `src/kohdalab/api/device_requirements.py`
  Shared required/missing device checks for Signal Monitor, TRKR, SRKR, STRKR,
  and SRKR 2D.
- `src/kohdalab/api/scan_limits.py`
  Hardware-derived scan-limit hints for delay-stage ps and scanner sample um.
- `src/kohdalab/api/devices/`
  Thin wrappers around interfaces for lock-in, delay-stage, and scanner
  operations. Lock-in wrappers include read helpers and setting write helpers.
- `src/kohdalab/api/notebook.py`
  Notebook-only formatting and live matplotlib helpers.
- `src/kohdalab/apps/trkr_gui.py`
  Everyday measurement-unit GUI.
- `src/kohdalab/apps/trkr_gui_advanced.py`
  Reference copy of the previous raw/interface coordinate GUI. It is not part
  of the current operator workflow.

## Config Shape

Two main config profiles exist:

- `config/kikuchi.json`
  CONEXCC scanner setup with TRA12CC actuators.
- `config/default.json`
  CONEXAGAP scanner setup with AG-M100D actuators on shared COM port axes U/V.

Normalized config contains:

```text
profile
instruments.lockin
instruments.delay_stage
instruments.scanner
measurements.move_abs
measurements.signal_monitor
measurements.trkr
measurements.srkr
measurements.strkr
measurements.srkr_2d
```

Scanner conversion uses:

```json
"sample_um_per_unit": 582.0
```

Legacy scanner scale keys such as `sample_um_per_actuator_mm` and
`sample_um_per_actuator_deg` are compatibility inputs and are normalized to
`sample_um_per_unit` when possible.

Measurement defaults live in `api.config.DEFAULT_MEASUREMENTS`.

## Coordinates

Use these public coordinate names:

```text
measurement
interface
instrument
```

Legacy aliases are still normalized:

```text
control -> interface
device  -> instrument for delay-stage coordinates
device  -> interface for scanner coordinates
```

Delay stage:

```text
measurement: t (ps)
interface:   delay_stage (mm)
instrument:  delay_stage (pulse)
```

Scanner:

```text
measurement: x/y sample position (um)
interface:   scanner actuator position (mm or deg)
instrument:  compatibility alias for interface
```

For scanners, new code and docs should prefer `interface` for actuator mm/deg.
`instrument`/`device` remain accepted input aliases, but scanner plans and move
rows normalize them to `interface` because there is no separate scanner
pulse/raw coordinate exposed by this API.

Hardware home/origin belongs to the control layer. Measurement coordinates use
the middle of each device's configured min/max travel as zero unless an
explicit origin is configured.

## Row Schemas

Rows are plain dictionaries with stable field order. All measurements use the
same canonical schema:

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

Signal Monitor uses `fast_axis=elapsed_s`. TRKR uses `fast_axis=t`. SRKR uses
`fast_axis=x/y`. STRKR and SRKR 2D also set `slow_axis`. The old `scan_axis`
and generic `target` columns are intentionally gone.

## Connection Model

`DeviceSession` keeps three connected-handle maps:

```text
lockins
delay_stages
scanners
```

It also supports `connected_devices()` for API/GUI requirement checks.

`auto_connect=True` is the default for notebooks and CLI convenience. With
`auto_connect=False`, read/move operations raise `RuntimeError` if a required
handle is missing. Explicit `connect_device()` and `connect_all()` still work
in both modes.

Lock-in setting writes follow the same policy through
`Experiment.set_lockin_settings()` and `DeviceSession.set_lockin_settings()`.

`DeviceSession` serializes hardware I/O per device reference with reentrant
locks. This prevents overlapping VISA/GPIB calls to the same lock-in, while
still allowing unrelated devices, such as a moving delay stage and lock-in live
status reads, to run at the same time.

Move and measurement status callbacks use constants from `kohdalab.api.status`.
Manual moves and measurement-owned moves emit the same movement strings, so
apps can call `moving_axis_from_status(status)` and update the relevant live
axis display without duplicating string parsing.

Interface modules also maintain module-level connection caches:

```text
_LOCKIN_CONNECTIONS
_DELAY_STAGE_CONNECTIONS
_SCANNER_CONNECTIONS
_SCANNER_SERIALS
```

The scanner serial cache allows CONEXAGAP X/Y axes to share one serial port.
Disconnecting one scanner only closes the serial port when no remaining scanner
uses it.

Delay-stage wrappers cache the controller microstep division after the first
successful query. Position conversion then avoids extra `?S`/`?MS` controller
queries during motion polling.

## Current GUI Surface

`trkr_gui.py` is the everyday operator GUI. It:

- loads/saves normalized API config
- connects all devices or individual devices
- reads live status
- sets measurement origins from current position
- moves delay/scanner axes in measurement units
- runs Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D
- saves rows with API row order and output formatting
- uses `Experiment(..., auto_connect=False)`
- refuses Start when `Experiment.missing_devices(...)` reports missing handles

Hardware-facing GUI work is routed through Qt workers:

- `LiveStatusWorker` for live status and lock-in-only polling during moves
- `DeviceCommandWorker` as the long-lived connect/disconnect/initialize,
  wait-time, lock-in-setting, and shutdown-disconnect command actor
- `MoveWorker` for manual delay-stage/scanner motion and progress signals
- `ResourceListWorker` for VISA resource and serial-port enumeration
- `MeasurementWorker` for Signal Monitor/TRKR/SRKR/STRKR/SRKR 2D runs

The main Qt thread should update widgets and build runtime config, but it should
not perform blocking device I/O directly. When a device is moving, that device's
worker owns its polling loop; unrelated lock-in live values may continue through
the live worker.

Window close uses the same device command actor to disconnect devices
asynchronously instead of calling `disconnect_all()` on the Qt main thread.

The everyday GUI only exposes ps and um. The previous raw/interface GUI remains
in the tree for reference, but it is not exposed as a script entry point.

## CLI Surface

`kohdalab-cli` supports:

```powershell
kohdalab-cli signal-monitor
kohdalab-cli trkr
kohdalab-cli srkr --axis x
kohdalab-cli strkr --fast-axis t --slow-axis x
kohdalab-cli srkr-2d --fast-axis x --slow-axis y
kohdalab-cli move-abs --axis t --coordinate measurement --value 0
```

TRKR/SRKR/STRKR/SRKR 2D CLI commands build scan plans from config before calling
`Experiment.run_*()`. The CLI keeps the default `auto_connect=True` behavior.

## Supported Hardware In Code

Lock-in controllers:

```text
SR7265, SR830, LI5640, SR5210
```

Delay-stage controllers:

```text
SHOT302GS, GSC01
```

Scanner controllers:

```text
CONEXCC, CONEXAGAP
```

Actuator/stage metadata is loaded from:

```text
src/kohdalab/instruments/scanner/actuator.toml
src/kohdalab/instruments/delay_stage/stages.toml
```

## Verification Status

The codebase has hardware-free pytest coverage for:

- config normalization and legacy compatibility
- scan range generation
- scanner conversion
- scan-limit hints
- device requirement resolution
- auto-connect policy
- session ownership during measurement runs
- row schema/order helpers
- lock-in controller parsing/settings behavior
- interface connection cache behavior for lock-in, delay-stage, and scanner,
  including shared CONEXAGAP serial lifetime
- GUI pure helpers and worker behavior

Recommended software verification after code changes:

```powershell
uv run python -m compileall src/kohdalab
uv run python -m kohdalab.api.cli --help
uv run pytest
```

Manual/hardware verification remains necessary for:

- everyday GUI operation using `docs/hardware_smoke_test.md`
- real device connection and initialization
- actual Signal Monitor/TRKR/SRKR/STRKR/SRKR 2D runs
- notebooks against connected instruments

## Remaining Work

1. Exercise the everyday GUI with `docs/hardware_smoke_test.md`.
2. Update notebooks and docs from any hardware-specific findings.

## Design Preference

Keep the public surface simple. New user code should usually need only:

```python
from kohdalab.api import Experiment, load_config
```

The GUI can expose many controls, but workflow logic should stay behind the API
rather than being duplicated across apps and notebooks.
