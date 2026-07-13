# KohdaLab API Usage

Japanese version: [`api_usage_ja.md`](api_usage_ja.md).

This is the practical reference for the current public API used by notebooks,
scripts, the CLI, and the TRKR GUI.

## Entry Point

Use `Experiment` from `kohdalab.api`.

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

config = load_config("config/kikuchi.json")
experiment = Experiment(config)

plan = trkr_plan_from_config(config)
rows = experiment.run_trkr(plan=plan)
```

Equivalent constructor:

```python
experiment = Experiment.from_config(
    "config/kikuchi.json"
)
```

`Experiment` owns one long-lived `DeviceSession`. Device reads, moves, and
measurement runs all go through that session.

## Connection Policy

The API and CLI default allows automatic connection:

```python
experiment = Experiment(config)  # auto_connect=True
```

With `auto_connect=True`, reads and moves can connect a missing device
automatically.

For deterministic cleanup, use `Experiment` or `DeviceSession` as a context
manager. `close()` is equivalent to `disconnect_all()` and is idempotent:

```python
with Experiment(config) as experiment:
    experiment.connect_all()
    rows = experiment.run_trkr()
```

The context manager also closes on a body exception. If measurement code and
cleanup both fail, the measurement exception remains primary and the cleanup
failure is attached as an exception note.

`connect_all()` is transactional. If a later device cannot connect, every
lease acquired by that call is released in reverse order, while connections
that existed before the call remain owned. Rollback attempts every acquired
device; any rollback failures are attached to the original connection error as
an exception note.

Initialization also registers its connection lease before homing or origin
movement and passes that exact handle through the service layer. If
initialization fails, a lease acquired by that call is released; a connection
that existed beforehand is retained. `initialize_xy()` applies the same rule
across both axes. Physical motion already completed before an error cannot be
rolled back.

`connected_devices()` checks each live wrapper instead of reporting only
session-dictionary membership. A false result or an exception from
`is_connected()` is treated as disconnected. Device operations reject stale
handles with an explicit disconnect/reconnect instruction, while disconnect
remains available to clean up the stale lease.

Sessions sharing one cached handle also share its reentrant I/O lock. Complete
session-level operations against that handle are serialized across sessions,
including connect and disconnect, while operations on unrelated hardware can
still run concurrently.

Changing measurement or output settings through `experiment.config` is allowed
while devices are connected. Changing or removing the config of a connected
instrument is rejected; disconnect it first so an existing handle can never be
used with a different resource, port, controller, or actuator definition.
`disconnect_all()` attempts every connected device even if one close operation
fails, then raises one error containing all failed references.
When multiple `DeviceSession` instances resolve to the same cached hardware
handle, each session owns a lease. Disconnecting one session releases only its
lease; the physical connection closes after the final owner disconnects.
Repeated `connect_device()` calls in one session are idempotent.
All owners of one physical target must use exactly the same instrument config;
a conflicting session is rejected before device I/O and reports the differing
top-level fields. A different config can be used after the final lease closes.
The session also pins its own per-device config at connection time. Mutating a
nested instrument config in place is detected before later device operations;
disconnect still uses the pinned resource or port so the original connection
cannot be orphaned.

Delay-stage measurement coordinates require a stable physical zero. Known
stage profiles derive it from the configured minimum/maximum travel midpoint.
Custom stages without a maximum limit must set finite `zero_pos_mm`; the API no
longer derives a moving zero from the current position. Instrument-coordinate
targets are integer pulses—fractional pulses and non-finite targets are
rejected before a controller command is sent.

Scanner conversion and initialization use the same origin precedence:
finite explicit `origin_pos`, otherwise the finite `min_pos`/`max_pos`
midpoint, otherwise zero. An explicit origin must remain inside configured
limits. Scanner targets, hardware readings, scale/origin values, and software
hysteresis distances must be finite. Hysteresis enablement must be boolean and
its direction must be a supported positive/negative approach alias.

Config loading canonicalizes lock-in models, controller names, stage names,
and actuator names, then checks them against the packaged driver/catalog data.
Stage/controller and actuator/controller compatibility is validated before a
session is created. Duplicate lock-in resources, duplicate delay-stage
controller/port pairs, and duplicate scanner controller/port/axis tuples are
rejected because they could expose one cached handle through independent
session locks. Measurement-specific device keys must resolve to existing
instrument entries.

The GUI uses explicit connection mode:

```python
experiment = Experiment(config, auto_connect=False)
```

With `auto_connect=False`, reads and moves require an already connected handle.
If a handle is missing, the API raises `Device not connected: ...`.

Explicit connect operations always work:

```python
experiment.connect_device("lockin.main")
experiment.connect_device("delay_stage.t")
experiment.connect_device("scanner.x")
experiment.connect_all()
```

Useful reference aliases:

```text
signal, lockin          -> lockin.<only key or main>
delay, delay_stage, t   -> delay_stage.<only key or t>
x, scanner_x            -> scanner.x
y, scanner_y            -> scanner.y
```

## Required Devices

The API owns measurement requirements:

```python
experiment.required_devices("signal_monitor")
experiment.required_devices("trkr")
experiment.required_devices("srkr", axis="x")
experiment.required_devices("strkr", fast_axis="t", slow_axis="x")
experiment.required_devices("srkr_2d", fast_axis="x", slow_axis="y")

missing = experiment.missing_devices("trkr")
```

Defaults:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

Config keys such as `lockin_key`, `delay_stage_key`, and SRKR `scanner_keys`
are respected.

## Live Status

Read the current connected-device map, positions, lock-in signal, settings, and
overload in one API call:

```python
status = experiment.read_live_status()
print(status.position.t_ps, status.signal, status.lockin_overload)
```

`LiveStatus` fields:

```text
connected
position
signal
lockin_settings
lockin_overload
```

## Scan Plans

Use scan plans for TRKR, SRKR, STRKR, and SRKR 2D. A plan keeps user-facing
corrected targets and actual absolute move points paired.

```python
from kohdalab.api import (
    srkr_2d_plan_from_config,
    srkr_plan_from_config,
    strkr_plan_from_config,
    trkr_plan_from_config,
)

trkr_plan = trkr_plan_from_config(config)
srkr_x_plan = srkr_plan_from_config(config, axis="x")
strkr_plan = strkr_plan_from_config(config, fast_axis="t", slow_axis="x")
srkr_2d_plan = srkr_2d_plan_from_config(config, fast_axis="x", slow_axis="y")
```

Explicit values:

```python
from kohdalab.api import srkr_2d_plan, srkr_plan, strkr_plan, trkr_plan

trkr = trkr_plan(
    minimum_ps=-50.0,
    maximum_ps=300.0,
    step_ps=5.0,
    t_zero_ps=-122.0,
    coordinate="measurement",
)

srkr = srkr_plan(
    axis="x",
    minimum_um=-30.0,
    maximum_um=30.0,
    step_um=1.0,
    zero_by_axis={"x": 61.5, "y": 477.0},
    coordinate="measurement",
)

strkr = strkr_plan(
    fast_axis="t",
    slow_axis="x",
    ranges={
        "t": {"min": -50.0, "max": 300.0, "step": 5.0},
        "x": {"min": -30.0, "max": 30.0, "step": 1.0},
        "y": {"min": -30.0, "max": 30.0, "step": 1.0},
    },
    zero_by_axis={"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0},
    return_to_zero={"fast_axis": True, "slow_axis": True},
)

srkr_2d = srkr_2d_plan(
    fast_axis="x",
    slow_axis="y",
    ranges={
        "x": {"min": -30.0, "max": 30.0, "step": 1.0},
        "y": {"min": -30.0, "max": 30.0, "step": 1.0},
    },
    zero_by_axis={"t_ps": -122.0, "x_um": 61.5, "y_um": 477.0},
    return_to_zero={"fast_axis": True, "slow_axis": True},
)
```

`Scan2DPlan` exposes `fast_point_count`, `slow_point_count`, and
`total_points`, which GUI progress and ETA code use instead of inspecting list
lengths directly.

Coordinate aliases are normalized:

```text
measurement
interface   (legacy alias: control)
instrument  (legacy alias: device; scanner/SRKR compatibility alias for interface)
```

## Scan Limits

For GUI hints or notebook validation, use the exported scan-limit helpers:

```python
from kohdalab.api import delay_stage_scan_limits, scanner_scan_limits

t_limits = delay_stage_scan_limits(
    stage="SGSP46-500",
    direction=1,
    t_zero_ps=-122.0,
)
x_limits = scanner_scan_limits(
    actuator="TRA12CC",
    sample_um_per_unit=582.0,
    zero_um=61.5756,
)
```

The helpers return `ScanLimits(minimum, maximum, minimum_step, unit)`.

## Running Measurements

Signal Monitor:

```python
from kohdalab.api import signal_monitor_plan_from_config

plan = signal_monitor_plan_from_config(config)
rows = experiment.run_signal_monitor(plan=plan)
```

You can still pass `interval_s` and `n_points` directly. Explicit values
override the plan values when both are supplied.

TRKR:

```python
plan = trkr_plan_from_config(config)
rows = experiment.run_trkr(plan=plan)
```

SRKR:

```python
plan = srkr_plan_from_config(config, axis="x")
rows = experiment.run_srkr(plan=plan)
```

STRKR:

```python
plan = strkr_plan_from_config(config, fast_axis="t", slow_axis="x")
rows = experiment.run_strkr(plan=plan)
```

SRKR 2D:

```python
plan = srkr_2d_plan_from_config(config, fast_axis="x", slow_axis="y")
rows = experiment.run_srkr_2d(plan=plan)
```

For notebook progress or live plots, pass callbacks:

```python
from kohdalab.api import format_point, make_trkr_live_update

live_update = make_trkr_live_update(y_key="R_V")

def on_point(point):
    print(format_point(point, axis_key="t_cor_ps"))
    live_update(point)

rows = experiment.run_trkr(plan=plan, on_point=on_point)
```

When `Experiment.run_*()` is used, the existing `experiment.session` is reused
and left connected. Standalone functions such as `run_trkr(config, ...)` create
a temporary session when no session is supplied, then disconnect it at the end.

## Moving Devices

Delay stage:

```python
position = experiment.move_delay_stage(0.0, coordinate="measurement")
```

Scanner:

```python
position = experiment.move_scanner("x", 10.0, coordinate="measurement")
```

Both move methods accept optional `on_status` and `on_position` callbacks.
`on_status` receives centralized strings from `kohdalab.api.status`, and
drivers call `on_position` from their polling loop while motion is in progress.

```python
from kohdalab.api import STATUS_MOVING_DELAY_STAGE, moving_scanner_status

def on_status(status):
    print(status)

def on_position(row):
    print(row)

experiment.move_delay_stage(10.0, coordinate="measurement", on_status=on_status, on_position=on_position)
experiment.move_scanner("x", 5.0, coordinate="measurement", on_status=on_status, on_position=on_position)

assert STATUS_MOVING_DELAY_STAGE == "moving delay stage"
assert moving_scanner_status("x") == "moving scanner x"
```

Measurement functions also use the same status channel. Common values are
`STATUS_RUNNING`, `STATUS_WAITING`, `STATUS_READING_LOCKIN`,
`STATUS_SLOW_AXIS_READY`, and `STATUS_STOPPED`. Use
`moving_axis_from_status(status)` when a UI needs to map motion status back to
`t`, `x`, or `y`.

Coordinate names:

```text
measurement: delay ps, scanner sample um
interface:   delay-stage mm, scanner actuator mm/deg
instrument:  delay-stage pulse
```

For scanners, prefer `measurement` for sample um and `interface` for actuator
mm/deg. `instrument` and `device` are still accepted for compatibility, but
scanner/SRKR plans normalize them to `interface` because the wrapped scanner
controllers do not expose a separate pulse/raw coordinate in this API.

## Lock-In Settings

Read current lock-in settings:

```python
settings = experiment.read_lockin_settings("lockin.main")
```

Apply settings through the connected `Experiment` session:

```python
applied = experiment.set_lockin_settings(
    "lockin.main",
    sensitivity=100e-6,
    time_constant=1.0,
    coupling="AC",
    slope=24,
)
```

Supported keyword arguments are:

```text
sensitivity
time_constant
ac_gain
coupling
slope
```

Only supplied values are written. The returned dictionary contains the settings
that were actually written, read back through the lock-in interface. Unsupported
model-specific writes, such as `ac_gain` on models without AC gain, raise from
the underlying driver.

## Row Schemas

Rows are plain dictionaries, but the field order is centralized in
`kohdalab.api.measurement_rows`.

All measurements use one canonical row order:

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

Signal Monitor uses `fast_axis=elapsed_s` and `target_elapsed_s`.
TRKR uses `fast_axis=t` and `target_t_cor_ps`.
SRKR uses `fast_axis=x` or `fast_axis=y` and the corresponding
`target_x_cor_um` or `target_y_cor_um`.
STRKR and SRKR 2D also set `slow_axis` and carry targets for both scanned axes.

`scan_axis` and generic `target` are no longer emitted.

Schema helpers are exported:

```python
from kohdalab.api import (
    MEASUREMENT_FIELDS,
    SIGNAL_MONITOR_FIELDS,
    SRKR_2D_FIELDS,
    TRKR_FIELDS,
    SRKR_FIELDS_BY_AXIS,
    STRKR_FIELDS,
    fields_for_row,
    fields_for_rows,
    output_row,
    output_rows,
    scan2d_row,
    signal_monitor_row,
    trkr_row,
    srkr_row,
)
```

Use `output_rows()` or `output_row()` when writing CSV manually; voltage fields
are formatted in scientific notation.

Measurement timestamps use RFC 3339 UTC with a `Z` suffix. Every measurement
CSV is accompanied by `<name>.csv.meta.json`. The sidecar records the run ID,
redacted config snapshot and SHA-256, software versions, expected and written
point counts, terminal status (`completed`, `stopped`, `failed`, or
`interrupted`), and the final CSV SHA-256. Use `write_measurement_rows()` for a
manual export that must regenerate a matching sidecar.

Measurement runs open their output CSV exclusively. If either the CSV or its
sidecar already exists, the run fails before point iteration and therefore
before measurement device I/O; existing data is never truncated implicitly.
Automatic filename suffixes include microseconds. `write_measurement_rows()`
also refuses replacement by default. Pass `overwrite=True` only for an
explicit replacement such as GUI **Save Now**; that path writes and fsyncs a
temporary CSV before atomically replacing the destination and regenerating its
hash-matched sidecar.

## CLI

Run measurements from PowerShell:

```powershell
kohdalab-cli --config config\kikuchi.json signal-monitor
kohdalab-cli --config config\kikuchi.json trkr
kohdalab-cli --config config\kikuchi.json srkr --axis x
kohdalab-cli --config config\kikuchi.json strkr --fast-axis t --slow-axis x
kohdalab-cli --config config\kikuchi.json srkr-2d --fast-axis x --slow-axis y
```

Move one axis:

```powershell
kohdalab-cli --config config\kikuchi.json move-abs --axis t --coordinate measurement --value 0
kohdalab-cli --config config\kikuchi.json move-abs --axis x --coordinate measurement --value 10
```

The CLI prints start/status/point progress and writes CSV rows to each
measurement's configured output path. It uses the default `auto_connect=True`
policy. The final `Saved` message is printed only after device cleanup succeeds.
Exit codes are `0` for a complete run, `1` for runtime or cleanup failure, `2`
for invalid arguments/configuration, and `130` for keyboard interruption.

## Notebooks

The maintained notebooks mirror the same public API entry points:

```text
notebook/move_abs_notebook.ipynb
notebook/signal_monitor_notebook.ipynb
notebook/trkr_notebook.ipynb
notebook/srkr_notebook.ipynb
notebook/strkr_notebook.ipynb
notebook/srkr_2d_notebook.ipynb
```

They use `Experiment`, scan-plan builders, `format_point()`, and notebook live
plot helpers. The maintained notebooks explicitly use `auto_connect=False`;
review the selected config and call `experiment.connect_all()` before a run.

## GUI

Run the everyday desktop GUI:

```powershell
uv run kohdalab-gui
```

or:

```powershell
uv run python -m kohdalab.apps.trkr_gui
```

The GUI creates `Experiment(..., auto_connect=False)`. Measurement start checks
`Experiment.missing_devices(...)` and refuses to run until the required handles
have been explicitly connected in the GUI.

The GUI routes blocking device work through Qt workers. Live status,
connect/disconnect, initialize, lock-in wait-time reads, manual moves, and
measurements all return results to the main thread by signal.

After GUI/API changes, run the manual checklist in
`docs/hardware_smoke_test.md` against real hardware.
