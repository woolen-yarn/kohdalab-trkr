# TRKR GUI

Japanese version: [`trkr_gui_ja.md`](trkr_gui_ja.md).

`src/kohdalab/apps/trkr_gui.py` is the everyday desktop GUI for KohdaLab
TRKR/SRKR operation. It is deliberately measurement-unit focused: operators work
with delay in ps and sample scanner position in um.

Run it with:

```powershell
uv run kohdalab-gui
```

or:

```powershell
uv run python -m kohdalab.apps.trkr_gui
```

For hardware verification after changes, use
[`hardware_smoke_test.md`](hardware_smoke_test.md). A Japanese version is
available at [`hardware_smoke_test_ja.md`](hardware_smoke_test_ja.md).

## Core Rule

The GUI is a thin operator surface over `kohdalab.api.Experiment`.

- Config editing stays in the GUI.
- Device ownership and measurement execution stay in `Experiment` and
  `DeviceSession`.
- Measurement runs reuse the already connected GUI `Experiment`.
- The GUI creates `Experiment(..., auto_connect=False)` and never starts a run
  by silently auto-connecting missing devices.
- GUI widgets do not perform hardware reads or motion directly. Hardware-facing
  work is routed through Qt workers and returns to the UI by signal.

## Layout

The window has three main panes.

### Left Pane

The left pane is a scrollable device/session panel. It is sized to about 20% of
the window width with a 384 px minimum.

Sections:

- `Session`
  Config path, Browse, Load, Save, Connect All, Disconnect All, and Read Live.
- `Lock-in`
  Device settings expander, Connect/Disconnect buttons, Overload, Sensitivity,
  TC, Frequency, and X/Y/R/Theta live values.
- `Delay Stage`
  Device settings expander, Connect/Disconnect/Initialize buttons, live `t`,
  origin/offset, corrected `t_cor`, absolute Move, and corrected Move.
- `Scanner X`
  Device settings expander, Connect/Disconnect/Initialize buttons, live `x`,
  origin/offset, corrected `x_cor`, absolute Move, and corrected Move.
- `Scanner Y`
  Same shape as Scanner X.

Motion rows are split between live position/origin state and move controls.

### Center Pane

The center pane contains measurement settings, output/run controls, and plots.
Measurement runs and **Save Now** write both the CSV and a matching
`.csv.meta.json` provenance sidecar. Row timestamps are UTC and the sidecar's
CSV SHA-256 can be used to detect later modification.
Runs never overwrite an existing CSV or sidecar. **Save Now** is an explicit
replacement operation and uses a temporary file plus atomic rename.

Tabs:

- `Signal Monitor`
- `TRKR`
- `SRKR`
- `STRKR`
- `SRKR 2D`

Each tab has its measurement settings on the left and Output/Run controls on
the right. Output settings are stored per measurement mode; switching tabs swaps
the directory/file/auto-suffix fields.

Run controls:

- Start
- Stop
- Save Now

There is no visible Clear Plot button. Starting a measurement clears only the
data owned by that run:

- Signal Monitor clears Signal Monitor rows.
- TRKR clears TRKR rows.
- SRKR clears only the active fast axis and preserves rows for the other axis.
- STRKR and SRKR 2D clear their own 2D rows.

### Right Pane

The right pane is sized to about 20% of the window width and can be collapsed
with the vertical `<` / `>` toggle.

It contains:

- Log output
- Snapshot table

The Log mirrors explicit GUI messages plus stdout/stderr, so messages printed
by connection helpers also appear in the GUI.

## Device Settings

Device settings live inside each device block behind a local `Device` expander.

Supported lock-in models in the GUI:

```text
SR7265, SR830, LI5640, SR5210
```

Supported delay-stage controllers:

```text
SHOT302GS, GSC01
```

Delay-stage stage choices are loaded from
`src/kohdalab/instruments/delay_stage/stages.toml` and filtered by controller.

Supported scanner controllers and actuator choices are loaded from
`src/kohdalab/instruments/scanner/actuator.toml`:

```text
CONEXCC   -> TRA12CC
CONEXAGAP -> AG-M100D
```

The scanner scale field label follows the actuator unit. CONEXCC/TRA12CC shows
`sample um / mm`; CONEXAGAP/AG-M100D shows `sample um / deg`.

When both scanner axes use `CONEXAGAP`, the GUI synchronizes the X/Y COM port
fields because the hardware shares one serial connection. The interface layer
also reuses the same `serial.Serial` handle internally.

Connect, Disconnect, Initialize, manual Move, lock-in wait-time reads, and live
status reads run through GUI worker objects. They all operate on the connected
GUI `Experiment` session and send results back to the main Qt thread by signal.
The resource Refresh buttons also enumerate VISA resources and COM ports in a
worker thread so slow hardware discovery does not freeze the window.
Lock-in setting writes are available in the public API, but the everyday GUI
currently exposes lock-in settings as read-only live status.

## Connection And Run Rules

Required connections before Start:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

The required-device rule comes from the API:

```python
Experiment.required_devices(...)
Experiment.missing_devices(...)
```

If a required device is missing, Start fails with a message instead of
connecting implicitly.

The GUI stores connected handles in `self.experiment.session`:

```text
lockins
delay_stages
scanners
```

Measurement workers receive the existing `Experiment` object and call
`Experiment.run_signal_monitor()`, `Experiment.run_trkr()`,
`Experiment.run_srkr()`, `Experiment.run_strkr()`, or
`Experiment.run_srkr_2d()`.

## Worker Model

The GUI uses a small actor-style worker set so slow hardware operations do not
block Qt's main event loop.

- `LiveStatusWorker`
  Reads full live status when idle. During a manual move it reads only lock-in
  settings/signal/overload so the moving serial device is not touched twice.
- `DeviceCommandWorker`
  Long-lived command actor for Connect, Disconnect, Initialize, `Use TC*4`,
  API-level lock-in setting writes, and shutdown disconnect.
- `MoveWorker`
  Handles manual delay-stage/scanner moves and forwards motion progress through
  position signals.
- `ResourceListWorker`
  Handles VISA resource and serial-port enumeration for Refresh buttons and
  startup resource loading.
- `MeasurementWorker`
  Owns Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D runs.

The moving device is polled only by its owning worker. For example, during a
manual X move, X position updates come from `MoveWorker`, while lock-in live
values continue through `LiveStatusWorker`.

The API session also serializes I/O per device reference. Repeated `Use TC*4`
clicks and lock-in live polling cannot issue simultaneous VISA/GPIB operations
against `lockin.main`; one command waits for the other to finish.

Measurement and manual motion use the same status API. Motion status strings
come from `kohdalab.api.status`, and the GUI maps them back to `t`, `x`, or
`y` with `moving_axis_from_status()` so live position labels can show
`Moving...` during both manual moves and measurement-owned moves.

Manual move clicks are debounced briefly at move start and completion, so queued
double-clicks do not immediately send a second command while the controller is
still settling.

## Locking During Runs

The GUI locks only controls for devices actively moved by the running
measurement:

- Signal Monitor: no motion-device controls are locked
- TRKR: delay-stage controls are locked
- SRKR: only the scanner axis being scanned is locked
- STRKR/SRKR 2D: only the fast and slow scan axes are locked

Unrelated manual controls stay available during long measurements.

## Origins And Coordinates

The everyday GUI only exposes measurement units:

```text
t: ps
x/y: um
```

Origin buttons set:

```text
t_zero_ps
x_zero_um
y_zero_um
```

Corrected move controls add the current origin to the corrected value, then
perform a measurement-coordinate absolute move.

Saved configs write:

```text
measurements.move_abs.zero.t_ps
measurements.move_abs.zero.x_um
measurements.move_abs.zero.y_um
```

The GUI also keeps `measurements.move_abs.targets` for the last typed absolute
and corrected move values.

## Measurement Rows

Rows are written in measurement-oriented form. The GUI snapshot and Save Now
use the canonical ordering from `kohdalab.api.measurement_rows`.

All measurement rows use one canonical column order:

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

`scan_axis` and generic `target` are no longer emitted. Use `fast_axis`,
`slow_axis`, and the axis-specific `target_*` columns.

STRKR is a two-dimensional time-space scan. The fast/slow axes must combine
`t` with either `x` or `y`; the unused axis is left wherever the operator has
already moved it.

SRKR 2D is a two-dimensional space-space scan over `x` and `y`; `t` is left
wherever the operator has already moved it.

## Plot Behavior

Signal Monitor:

- two plots
- x-axis is `elapsed_s`
- y-axis is X/Y or R/Theta depending on plot mode

TRKR:

- two plots
- bottom x-axis is `t_cor_ps`
- top x-axis shows raw `t_ps`

SRKR:

- four plots
- columns are scan axes
  - left column: `x_cor_um`
  - right column: `y_cor_um`
- rows are selected signal channels
  - X/Y mode: top row X, bottom row Y
  - R/Theta mode: top row R, bottom row Theta
- top x-axis shows uncorrected `x_um` or `y_um`

STRKR and SRKR 2D:

- left column: two line plots of the current slow-axis line versus the fast axis
- right column: two heatmaps with fast axis on x and slow axis on y
- rows follow the selected signal mode, X/Y or R/Theta
- scans are non-serpentine; each slow-axis line is measured in the same fast
  min-to-max direction
- heatmaps are normalized by absolute maximum and displayed with a
  reversed-red/blue diverging lookup table
- ETA is shown only for STRKR/SRKR 2D and starts after the first full fast-axis
  line plus the next slow-axis move has completed

Plot data and point counters are retained per measurement tab. Switching tabs
redraws the shared plot area from the active mode's retained rows.

## Lock-in Display

Lock-in live status displays:

- Overload
- Sensitivity
- TC
- Frequency
- X/Y/R/Theta

TRKR and SRKR wait rows have `Use TC*4` buttons. If a stale VISA handle reports
`Invalid session handle`, the device command worker reconnects `lockin.main`
once and retries.

Rapid repeated clicks are ignored while a device command is already active.
Lock-in live status and `Use TC*4` share the same API-side lock, so they do not
collide on the GPIB/VISA session.

Lock-in settings are read and displayed only in the everyday GUI. The API still
provides `Experiment.set_lockin_settings()` for scripted or future operator
workflows.

## Shutdown

Closing the window starts an asynchronous shutdown path. The GUI stops live
polling, waits for active measurement/move/device commands to finish, then uses
the device command actor to run `disconnect_all()` off the Qt main thread.
