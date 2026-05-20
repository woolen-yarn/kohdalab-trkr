# Hardware Smoke Test

Japanese version: [`hardware_smoke_test_ja.md`](hardware_smoke_test_ja.md).

This checklist is for the everyday GUI (`kohdalab-gui`) after API or GUI
changes. It is intentionally short and conservative: confirm connection,
initialization, small moves, short runs, output files, and clean disconnects
before attempting a normal experiment.

Use the lab's active config profile:

```powershell
uv run kohdalab-gui
```

Known config profiles:

```text
config/default.json  CONEXAGAP / AG-M100D
config/kikuchi.json  CONEXCC / TRA12CC
```

## Record

```text
Date:
Operator:
Config file:
Scanner profile: CONEXCC / CONEXAGAP
Lock-in model:
Delay-stage controller:
Notes:
```

## Preflight

- [ ] Confirm the correct config file is loaded.
- [ ] Confirm lock-in resource, delay-stage port, and scanner ports match the
  hardware currently connected.
- [ ] For CONEXAGAP, confirm X and Y share the same COM port and use axes U/V.
- [ ] Confirm scanner scale labels match the actuator unit:
  `sample um / mm` for TRA12CC, `sample um / deg` for AG-M100D.
- [ ] Confirm output directory is writable and not an important raw-data
  folder that should be kept untouched.
- [ ] Confirm all motion ranges and current sample clearance are safe for small
  test moves.

## Connection

- [ ] Start the everyday GUI with `uv run kohdalab-gui`.
- [ ] Click `Read Live` before connecting. Expected: missing devices are
  reported cleanly, without freezing the GUI.
- [ ] Click the lock-in or COM-port `Refresh` buttons. Expected: the GUI stays
  responsive while VISA resources and serial ports are refreshed.
- [ ] Connect `Lock-in`. Expected: status becomes connected and X/Y/R/Theta
  display updates.
- [ ] Connect `Delay Stage`. Expected: status becomes connected and `t` live
  position updates.
- [ ] Connect `Scanner X`. Expected: status becomes connected and `x` live
  position updates.
- [ ] Connect `Scanner Y`. Expected: status becomes connected and `y` live
  position updates.
- [ ] Click `Disconnect All`, then connect with `Connect All`. Expected: all
  devices reconnect and the log shows normal reused/new connection messages.

## Initialization

- [ ] Click delay-stage `Initialize`. Expected: log shows initializing and
  moving to `t_ps=0`; live `t` updates after completion.
- [ ] Click scanner X `Initialize`. Expected: log shows scanner X initializing
  and moving to origin; live `x` updates after completion.
- [ ] Click scanner Y `Initialize`. Expected: log shows scanner Y initializing
  and moving to origin; live `y` updates after completion.
- [ ] During each initialization, confirm the matching Initialize/Move controls
  are disabled and re-enabled after completion.

## Origins And Manual Moves

- [ ] Click current-origin buttons for `t`, `x`, and `y`. Expected:
  corrected values are near zero.
- [ ] Perform one small absolute delay move in ps, then return to the prior
  value or to `t=0`.
- [ ] Perform one small corrected delay move. Expected: raw `t` and corrected
  `t_cor` update consistently.
- [ ] Click a delay-stage Move button repeatedly. Expected: duplicate clicks
  are ignored/debounced and no unexpected `?S` response error appears.
- [ ] Perform one small absolute scanner X move in um, then return.
- [ ] Perform one small corrected scanner X move. Expected: `x` and `x_cor`
  update consistently.
- [ ] Repeat the small absolute and corrected scanner move for Y.
- [ ] Confirm Snapshot rows contain `delay_stage_mm`, `delay_stage_pulse`, and
  `x_scanner_mm`/`x_scanner_deg` or `y_scanner_mm`/`y_scanner_deg` as
  appropriate.

## Short Measurement Runs

Use small ranges and point counts for smoke testing.

Signal Monitor:

- [ ] Set `n_points` to 3 and interval to a short value.
- [ ] Start Signal Monitor. Expected: plot updates, counter reaches 3, and no
  motion controls are unnecessarily locked.
- [ ] Click `Save Now`. Expected: CSV contains:
  `timestamp, measurement, fast_axis, target_elapsed_s, elapsed_s,
  X_V, Y_V, R_V, Theta_deg`.

TRKR:

- [ ] Set a short TRKR range around the current origin, for example 3 points.
- [ ] Confirm `Return to zero` is enabled for the first smoke run.
- [ ] Start TRKR. Expected: delay-stage controls lock, scanner controls remain
  available, plot updates, and the stage returns to zero at the end.
- [ ] Click `Save Now`. Expected: CSV contains:
  `measurement, fast_axis, target_t_cor_ps, t_cor_ps, t_ps, X_V, Y_V,
  R_V, Theta_deg, delay_stage_mm, delay_stage_pulse`.

SRKR:

- [ ] Select X axis and set a short SRKR range around the current origin, for
  example 3 points.
- [ ] Start SRKR X. Expected: scanner X controls lock, scanner Y and
  delay-stage controls remain available, plot updates, and X returns to zero.
- [ ] Repeat with Y axis. Expected: scanner Y controls lock and X rows remain
  visible when switching back to the SRKR tab.
- [ ] Click `Save Now`. Expected: CSV contains the active scanner axis fields,
  including `x_cor_um`/`x_um`/`x_scanner_*` or
  `y_cor_um`/`y_um`/`y_scanner_*`.

STRKR and SRKR 2D:

- [ ] Use tiny 2 x 2 ranges for the first smoke run.
- [ ] Start STRKR with `t` and one spatial axis. Expected: only the scanned
  delay/scanner controls lock; the unused axis is not moved by the run.
- [ ] Start SRKR 2D with X/Y. Expected: scanner X/Y controls lock and delay
  stage controls remain available.
- [ ] Confirm the left plots show the current fast-axis line and the right
  plots fill heatmaps with fast axis on x and slow axis on y.
- [ ] Confirm heatmaps use a red/blue diverging scale centered by absolute
  maximum normalization.
- [ ] Confirm ETA stays blank at the very beginning, then appears after the
  first fast-axis line and the next slow-axis move.

## Lock-In Wait And Recovery

- [ ] Click `Use TC*4` on TRKR wait. Expected: wait value updates from the
  connected lock-in time constant.
- [ ] Click `Use TC*4` on SRKR wait. Expected: wait value updates similarly.
- [ ] Click `Use TC*4` on STRKR and SRKR 2D wait. Expected: wait values update
  similarly.
- [ ] Click `Use TC*4` repeatedly while live status is active. Expected: the
  GUI ignores overlapping clicks and no VISA/GPIB listener error dialog appears.
- [ ] If a stale VISA session is suspected, disconnect/reconnect lock-in and
  retry `Use TC*4`. Expected: the GUI logs the reconnect attempt and recovers
  without restarting the application.

## Disconnect And Restart

- [ ] Click `Disconnect All`. Expected: all connected statuses clear and no
  unhandled exception appears in the log.
- [ ] Close the GUI. Expected: no worker thread warning or hang.
- [ ] While devices are still connected, close the GUI. Expected: the window
  enters closing state and disconnects devices without freezing.
- [ ] Reopen `uv run kohdalab-gui`.
- [ ] Load the same config. Expected: saved origin/output settings are present.
- [ ] Connect all once more. Expected: all devices reconnect normally.
- [ ] Disconnect all and close.

## Pass Criteria

- [ ] The everyday GUI opens and closes cleanly.
- [ ] Individual connect/disconnect and Connect All/Disconnect All work.
- [ ] Initialize buttons complete for delay stage and both scanners.
- [ ] Small absolute and corrected moves update live values consistently.
- [ ] Signal Monitor, TRKR, SRKR X/Y, STRKR, and SRKR 2D complete short runs.
- [ ] CSV headers match `kohdalab.api.measurement_rows`.
- [ ] Logs contain no unexpected tracebacks.
- [ ] Any hardware-specific issue is recorded with config file, controller,
  port/resource, operation, and the exact log message.
