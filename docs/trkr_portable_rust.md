# KohdaLab TRKR native application

Version 0.3.0 replaces the Python GUI and measurement backend with Rust.
Python is optional and used only by the thin Notebook client. The external
USB-GPIB/VISA helper processes are C/C++ and disclosed in REDISTRIBUTION.md.

## Devices and transport

- Lock-ins: SR5210, SR7265, SR830 and LI5640; GPIB primary addresses 1–30.
- Delay controllers: SHOT302GS and GSC01/GSC01A.
- Scanners: CONEX-CC (including TRA12CC) and CONEX-AGAP.
- Windows uses a system 64-bit VISA runtime when it enumerates the selected
  resource. Enumeration does not scan GPIB instrument addresses. Helpers run
  without a console window. A failed VISA operation is never retried on USB-GPIB.
- The separate native helper supports NI USB-GPIB-HS+ and already initialized
  Agilent 82357B adapters. Authorized initialization and appropriate device
  access/drivers are prerequisites; no firmware or driver installer is bundled.
- On Mac, choose the actual `/dev/cu.*` serial ports in Device settings.

Connecting does not home or move axes. Set controller, actuator, scale,
software hysteresis, direction, origin and limits for the real apparatus.
Unreferenced axes cannot start a scan until the operator resolves their state.
Effective delay-stage step is read from its microstep division after connection;
before connection only an explicitly marked full-step estimate is shown.

## Measurement and interface

Config remains above a device-only floating-scroll area. The left chevron hides
or restores the setup panel. The right panel is 300 logical pixels wide, and
only the centre grows during window resizing. All tabs share the same widths.
Output and Run frames have 12-pixel outer and between-frame margins.

Device headers display `----` while disconnected; connected names carry relevant
Moving/Waiting/Reading status without replacing numeric readouts. Step displays
acquired/total, including when the display retains only a limited plot history.
TRKR's plot toolbar selects Linear or Log (positive values only); raw signals
and CSVs are unaffected. Heatmaps use the frozen 256-colour Python RdBu reference.

SRKR 2D permits only x/y or u/v pairs, with a single shared theta editor beside u.
STRKR pairs t with one of x/y/u/v in either order. Targets and return movements
are validated before acquisition. Settling deadlines, motion-before-read order
and per-point CSV flush are preserved; live polling does not add reads during settling.

## Read-only demo

Windows: run `kohdalab-trkr-demo.exe`, or drop one CSV onto it.
Mac: open KohdaLab TRKR Demo.app. Load a CSV, Connect All (virtual), then Start.
The playback loops until Stop; Wait=0 gives fast replay. It cannot open hardware,
move an instrument, record a new CSV or overwrite its source. Only synthetic
example files are distributed.

## Notebook and automation

The stdio API accepts one JSON command per line. See `bindings/python` for the
standard-library Client and `notebook/` for six simulation-first examples.
`--profile-json FILE` validates a profile and exports hardware/plans without I/O.
`--stdio` emits state, phase, sample, live, error and finished events. Advisory
phase events are nonblocking; live events never count as acquisition samples.
Run requests are serialized, and Stop may be sent from another Python thread.

## Validation and release

CI builds all application/helper binaries from checked-in source on GitHub-hosted
runners. Windows portable/Setup and Mac application ZIP/DMG include source and
licenses. SHA256SUMS and BUILD-ORIGIN files accompany each platform. Signing
preparation is described in `docs/windows_code_signing.md`.

Simulation, cancellation, CSV replay and read-only instrument checks were run
during migration. Motion of the laboratory's unreferenced X axis was deliberately
not initiated; full physical scan validation remains an apparatus commissioning task.
No real movement is needed for repository tests or release builds.
