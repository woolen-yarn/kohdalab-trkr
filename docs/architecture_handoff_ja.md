# KohdaLab Architecture Handoff

English version: [`architecture_handoff.md`](architecture_handoff.md).

このメモは、現在の working tree の構成を次の作業者がすぐ把握できるように残すものです。

## Goal

notebook、desktop GUI、CLI、将来の app は、1つの public API 経由で実験装置を制御します。

```text
apps / notebooks / CLI / future apps
  -> kohdalab.api
  -> kohdalab.interfaces
  -> kohdalab.instruments
```

`kohdalab.api` が workflow layer です。GUI に workflow logic を重複させない方針です。

## Current Public API

中心 object は `Experiment` です。

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

`Experiment` は1つの `DeviceSession` を所有します。`Experiment.run_*()` は常にその session を measurement layer に渡し、run 後も接続を維持します。Signal Monitor、TRKR、SRKR、STRKR、SRKR 2D は対応する plan object を受け取ります。

standalone の `api.measurements.run_*()` も残しています。session が渡されない場合だけ temporary session を作り、最後に disconnect します。

## Important Modules

- `src/kohdalab/api/config.py`: config load/save、normalization、validation、output path、scan range
- `src/kohdalab/api/session.py`: `DeviceSession`、connected handle ownership、auto-connect policy、initialization、live status、move operation、device reference alias
- `src/kohdalab/api/experiment.py`: notebook/GUI/CLI 向け public facade
- `src/kohdalab/api/measurements.py`: Signal Monitor、TRKR、SRKR、STRKR、SRKR 2D workflow
- `src/kohdalab/api/scan_plan.py`: `SignalMonitorPlan`, `TrkrPlan`, `SrkrPlan`, `StrkrPlan`, `Srkr2DPlan` と builder
- `src/kohdalab/api/measurement_rows.py`: canonical row field order、row constructor、CSV output formatting
- `src/kohdalab/api/status.py`: status constants と movement status helper
- `src/kohdalab/api/device_requirements.py`: required/missing device check
- `src/kohdalab/api/scan_limits.py`: delay-stage ps / scanner sample um の scan-limit hint
- `src/kohdalab/api/devices/`: interface wrapper。lock-in read/settings write、delay-stage/scanner operations
- `src/kohdalab/apps/trkr_gui.py`: everyday measurement-unit GUI
- `src/kohdalab/apps/trkr_gui_advanced.py`: 以前の raw/interface coordinate GUI の参考 copy。現在の operator workflow には含めない

## Config Shape

主な config profile:

- `config/kikuchi.json`: CONEXCC / TRA12CC
- `config/default.json`: CONEXAGAP / AG-M100D shared COM port

normalized config の主な key:

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

scanner conversion は `sample_um_per_unit` を使います。legacy の `sample_um_per_actuator_mm` / `sample_um_per_actuator_deg` は compatibility input として normalization されます。

## Coordinates

public coordinate names:

```text
measurement
interface
instrument
```

legacy aliases:

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

new code/docs では scanner actuator mm/deg に `interface` を使います。scanner の separate pulse/raw coordinate はこの API では公開していません。

## Row Schemas

全 measurement は同じ canonical schema を使います。

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

Signal Monitor は `fast_axis=elapsed_s`、TRKR は `fast_axis=t`、SRKR は `fast_axis=x/y`、STRKR/SRKR 2D は `fast_axis` と `slow_axis` を使います。旧 `scan_axis` と generic `target` は使いません。

## Connection Model

`DeviceSession` は connected handle map を持ちます。

```text
lockins
delay_stages
scanners
```

`auto_connect=True` は notebook/CLI 向け default です。GUI は `auto_connect=False` を使い、必要 handle が missing の場合は Start しません。

`DeviceSession` は device reference ごとの reentrant lock で I/O を直列化します。これにより同じ lock-in handle への VISA/GPIB command は overlap せず、delay stage moving 中でも lock-in live status のような unrelated I/O は継続できます。

move と measurement の status callback は `kohdalab.api.status` の constants/helper を使います。apps は `moving_axis_from_status(status)` で status を `t/x/y` に戻せます。

interface modules は module-level connection cache も持ちます。

```text
_LOCKIN_CONNECTIONS
_DELAY_STAGE_CONNECTIONS
_SCANNER_CONNECTIONS
_SCANNER_SERIALS
```

CONEXAGAP の X/Y axis は同じ serial port を共有できます。

## Current GUI Surface

`trkr_gui.py` は everyday operator GUI です。

- normalized API config の load/save
- all devices / individual device connect
- live status read
- current position から origin 設定
- measurement units で delay/scanner move
- Signal Monitor、TRKR、SRKR、STRKR、SRKR 2D
- API row order で Save Now / CSV output
- `Experiment(..., auto_connect=False)`
- `Experiment.missing_devices(...)` による Start guard

hardware-facing work は worker 経由です。

- `LiveStatusWorker`
- `DeviceCommandWorker`
- `MoveWorker`
- `ResourceListWorker`
- `MeasurementWorker`

main Qt thread は widget update と runtime config construction に集中し、blocking device I/O は実行しません。

## CLI Surface

```powershell
kohdalab-cli signal-monitor
kohdalab-cli trkr
kohdalab-cli srkr --axis x
kohdalab-cli strkr --fast-axis t --slow-axis x
kohdalab-cli srkr-2d --fast-axis x --slow-axis y
kohdalab-cli move-abs --axis t --coordinate measurement --value 0
```

CLI は config から scan plan を作って `Experiment.run_*()` を呼びます。default は `auto_connect=True` です。

## Verification Status

hardware-free pytest coverage:

- config normalization
- scan range generation
- scanner conversion
- scan-limit hints
- device requirement resolution
- auto-connect policy
- session ownership
- row schema/order helper
- status helper
- lock-in parsing/settings behavior
- interface connection cache behavior
- GUI pure helper and worker behavior

software verification:

```powershell
uv run ruff check src tests
uv run pytest
```

manual/hardware verification:

- `docs/hardware_smoke_test_ja.md` に沿った everyday GUI 確認
- real device connection / initialization
- Signal Monitor/TRKR/SRKR/STRKR/SRKR 2D
- connected instruments を使う notebooks

## Remaining Work

1. `docs/hardware_smoke_test_ja.md` で実機確認。
2. 実機固有の finding を docs/notebook に反映。

## Design Preference

public surface は小さく保ちます。新しい user code は基本的に次だけで始められるのが理想です。

```python
from kohdalab.api import Experiment, load_config
```

GUI は多くの controls を表示しても、workflow logic は API の背後に置きます。
