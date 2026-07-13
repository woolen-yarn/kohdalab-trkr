# API Cleanup Plan

English version: [`api_cleanup_plan.md`](api_cleanup_plan.md).

この文書は `kohdalab.api` surface の整理状況を追うためのメモです。現在の大きな方針は、workflow logic を API に寄せ、GUI/notebook/CLI はその薄い呼び出し側にすることです。

## Current Stable Shape

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

experiment = Experiment(load_config("config/kikuchi.json"))
plan = trkr_plan_from_config(experiment.config)
rows = experiment.run_trkr(plan=plan)
```

主要 module:

- `api/experiment.py`: GUI、notebook、CLI 向け public facade
- `api/session.py`: connected device handle、auto-connect policy、per-device lock
- `api/measurements.py`: Signal Monitor、TRKR、SRKR、STRKR、SRKR 2D workflow
- `api/scan_plan.py`: corrected target と actual move point を持つ plan/builder
- `api/measurement_rows.py`: canonical row schema、row constructor、CSV formatting
- `api/status.py`: status constants と movement status helper
- `api/device_requirements.py`: required/missing device rule
- `api/scan_limits.py`: hardware-derived scan limit hints
- `api/notebook.py`: notebook display/plot helper

## Completed Cleanup

### Row Schemas Are Centralized

Done:

- `api/measurement_rows.py` を追加
- `MEASUREMENT_FIELDS`, `SIGNAL_MONITOR_FIELDS`, `TRKR_FIELDS`, `SRKR_FIELDS_BY_AXIS`, `STRKR_FIELDS`, `SRKR_2D_FIELDS` を追加
- `signal_monitor_row()`, `trkr_row()`, `srkr_row()`, `scan2d_row()` を追加
- `fields_for_row()` / `fields_for_rows()` で CSV と snapshot の field order を統一

current schema:

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

旧 `scan_axis` と generic `target` は使わず、`fast_axis`, `slow_axis`, axis-specific `target_*` に統一しています。

### Scan Planning Is API-Owned

Done:

- `SignalMonitorPlan`, `TrkrPlan`, `SrkrPlan`, `Scan2DPlan`, `StrkrPlan`, `Srkr2DPlan` を追加
- explicit builder と config-derived builder を追加
- `Experiment.run_signal_monitor()`, `run_trkr()`, `run_srkr()`, `run_strkr()`, `run_srkr_2d()` が plan を受け取る
- STRKR/SRKR 2D は non-serpentine 2D scan
- `Scan2DPlan` は `fast_point_count`, `slow_point_count`, `total_points` を持つ

### Required Devices Are API-Owned

Done:

- `required_devices()` / `missing_devices()` を追加
- `Experiment.required_devices()` / `Experiment.missing_devices()` を追加
- GUI Start check は API に委譲

defaults:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

### Auto-Connect Policy Is Explicit

Done:

- `DeviceSession(auto_connect=True)` と `Experiment(auto_connect=True)` を追加
- API/CLI は default で auto-connect。maintained notebook は `auto_connect=False` を明示
- GUI は `Experiment(..., auto_connect=False)` を使い、明示接続済み handle だけで run
- supplied session は measurement function から disconnect しない
- standalone measurement function は session が渡されない場合だけ temporary session を作る

### GUI Hardware Access Is Worker-Routed

Done:

- live status、device command、manual move、measurement run を worker 化
- resource refresh も worker thread で実行
- `DeviceCommandWorker` は Connect/Disconnect/Initialize/Use TC/shutdown disconnect の command actor
- window close の `disconnect_all()` も main thread 外で実行
- manual move 中は動いている device を `MoveWorker` が所有し、lock-in live status は別 worker で継続

### DeviceSession Serializes Per-Device I/O

Done:

- `DeviceSession` に per-device reentrant lock を追加
- 同じ lock-in handle への signal/settings/overload/wait-time/settings-write は直列化
- delay stage や scanner move はその device lock だけを持つため、無関係な lock-in live status は継続可能
- `Use TC*4` 連打は GUI 側でも active command 中に無視

### Measurement Status API Is Centralized

Done:

- `api/status.py` を追加
- `STATUS_RUNNING`, `STATUS_WAITING`, `STATUS_READING_LOCKIN`, `STATUS_SLOW_AXIS_READY`, `STATUS_STOPPED` を追加
- `moving_scanner_status()`, `moving_axis_status()`, `moving_axis_from_status()` を追加
- manual move と measurement-owned move が同じ movement status を emit
- GUI は status から `t/x/y` を復元して該当 live label を `Moving...` にする

### Lock-In Setting Writes Are API-Level

Done:

- `api.devices.lockin.set_lockin_settings()` を追加
- `DeviceSession.set_lockin_settings()` と `Experiment.set_lockin_settings()` を追加
- `kohdalab.api` から export
- 通常 GUI は現状 read/display のみ。operator workflow が固まるまで編集 UI は出さない

### Interface Cache Behavior Is Tested

Done:

- lock-in、delay-stage、scanner connection cache の hardware-free tests を追加
- CONEXAGAP X/Y の shared serial lifetime を test
- stale-handle replacement、cache key separation、targeted disconnect、disconnect-all を test

### Hardware Smoke Test Checklist Is Documented

Done:

- `docs/hardware_smoke_test.md` と `docs/hardware_smoke_test_ja.md` を追加
- connect/disconnect、initialize、小さい move、Signal Monitor/TRKR/SRKR/STRKR/SRKR 2D、CSV header、Use TC recovery、clean shutdown を確認対象にした

## Current Design Rules

1. `Experiment` が connected `DeviceSession` を所有する。
2. GUI measurement は明示接続済み handle を使う。
3. standalone measurement function が temporary session を作るのは、session が渡されない場合だけ。
4. supplied session は measurement function から disconnect しない。
5. public row schema は explicit and stable。
6. workflow logic は API、GUI は operator surface。
7. device protocol quirks は `instruments/`、handle reuse は `interfaces/`。
8. hardware I/O は GUI main thread で実行しない。
9. same-device command は overlap させず、unrelated live status は可能なら継続する。

## Remaining Cleanup

残りは主に実機確認と、その結果の docs/notebook 反映です。

- `docs/hardware_smoke_test_ja.md` で everyday GUI を実機確認
- connected instruments を使う notebook を更新
- 実機固有の issue が出たら config、controller、port/resource、operation、log message を記録して docs に反映
