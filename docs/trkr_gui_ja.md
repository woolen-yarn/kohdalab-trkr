# TRKR GUI

English version: [`trkr_gui.md`](trkr_gui.md).

`src/kohdalab/apps/trkr_gui.py` は KohdaLab の通常運用向け desktop GUI です。operator は delay を ps、scanner position を sample um として扱います。

```powershell
uv run kohdalab-gui
```

または:

```powershell
uv run python -m kohdalab.apps.trkr_gui
```

実機確認には [`hardware_smoke_test_ja.md`](hardware_smoke_test_ja.md) を使います。

## Core Rule

GUI は `kohdalab.api.Experiment` の薄い operator surface です。

- config editing は GUI 側。
- device ownership と measurement execution は `Experiment` / `DeviceSession` 側。
- GUI は `Experiment(..., auto_connect=False)` を作り、missing device を勝手に auto-connect して Start しません。
- hardware read/move は main Qt thread では実行せず、worker から signal で戻します。

## Layout

window は左 device/session pane、中央 measurement/plot pane、右 log/snapshot pane の3分割です。

左 pane:

- `Session`: config path、Browse、Load、Save、Connect All、Disconnect All、Read Live
- `Lock-in`: Connect/Disconnect、Overload、Sensitivity、TC、Frequency、X/Y/R/Theta live values
- `Delay Stage`: live `t`、origin、corrected `t_cor`、absolute/corrected Move
- `Scanner X/Y`: live `x/y`、origin、corrected `x_cor/y_cor`、absolute/corrected Move

中央 pane の tabs:

- `Signal Monitor`
- `TRKR`
- `SRKR`
- `STRKR`
- `SRKR 2D`

各 tab は左に measurement settings、右に Output/Run controls を持ちます。Output の Directory/File/Auto suffix は measurement mode ごとに保持されます。Start 時には、その run が持つ rows だけを clear します。SRKR は active fast axis の rows だけを clear し、反対軸は残します。

右 pane は Log output と Snapshot table です。

## Device Settings

GUI が扱う lock-in model:

```text
SR7265, SR830, LI5640, SR5210
```

delay-stage controller:

```text
SHOT302GS, GSC01
```

scanner controller / actuator:

```text
CONEXCC   -> TRA12CC
CONEXAGAP -> AG-M100D
```

CONEXAGAP で X/Y が同じ COM port を共有する構成では、GUI も X/Y port field を同期します。interface layer も同じ serial handle を共有します。

Connect、Disconnect、Initialize、manual Move、Use TC、live status、resource refresh は worker thread で実行されます。lock-in settings write は API にはありますが、通常 GUI では現状 read/display のみにしています。

## Connection And Run Rules

Start 前に必要な接続:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

判定は API の `Experiment.required_devices()` / `Experiment.missing_devices()` にあります。missing device があれば、GUI は message を出して Start しません。

PC で使用しない機器種別は `instruments` から省略するか空 object にできます。GUI は省略状態を保持し、依存する motion 操作と測定を無効化して、config 読み込み時に log へ記録します。GUI の `Connect All` は接続できた機器を保持し、失敗した機器だけを skipped として表示します。API の `Experiment.connect_all()` は従来どおり失敗時 rollback の厳密な動作です。

## Worker Model

GUI は slow hardware operation で Qt main event loop を止めないように worker を使います。

- `LiveStatusWorker`: idle 時は full live status、manual move 中は lock-in only status を読む
- `DeviceCommandWorker`: Connect/Disconnect/Initialize、Use TC、lock-in setting write、shutdown disconnect
- `MoveWorker`: manual delay-stage/scanner move と position signal
- `ResourceListWorker`: VISA resource / serial port refresh
- `MeasurementWorker`: Signal Monitor、TRKR、SRKR、STRKR、SRKR 2D

同じ device reference への I/O は `DeviceSession` の per-device lock で直列化されます。たとえば `Use TC` と lock-in live status は同じ GPIB/VISA handle に同時アクセスしません。一方、delay stage が moving 中でも lock-in live status は読めます。

manual move と measurement 中の move は同じ status API を使います。`kohdalab.api.status.moving_axis_from_status()` で `t/x/y` に戻し、該当 live position label を `Moving...` にします。

## Locking During Runs

GUI は実際に動かす軸の controls だけを lock します。

- Signal Monitor: motion-device controls は lock しない
- TRKR: delay-stage controls
- SRKR: scan 中の scanner axis だけ
- STRKR/SRKR 2D: fast/slow scan axes だけ

関係ない manual controls は長い measurement 中でも使えます。

## Origins And Coordinates

通常 GUI は measurement units だけを表示します。

```text
t: ps
x/y: um
```

origin buttons は `t_zero_ps`, `x_zero_um`, `y_zero_um` を設定します。corrected move は current origin + corrected value を absolute target として measurement-coordinate move を実行します。

## Measurement Rows

保存 row は measurement-oriented schema です。

measurement run と **Save Now** は CSV と対応する `.csv.meta.json` provenance sidecar を保存します。row timestamp は UTC で、sidecar の CSV SHA-256 により保存後の変更を検出できます。
runは既存CSV/sidecarを上書きしません。**Save Now**だけが明示的な置換操作で、temporary fileからatomic renameします。

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

`scan_axis` と generic `target` は使いません。`fast_axis`, `slow_axis`, axis-specific な `target_*` columns を使います。

STRKR は `t` と `x/y` の時空間 2D scan、SRKR 2D は `x/y` の空間 2D scan です。scan しない軸は measurement 中に触らず、operator が事前に move した位置のままです。

## Plot Behavior

Signal Monitor:

- x-axis は `elapsed_s`
- y-axis は X/Y または R/Theta

TRKR:

- bottom x-axis は `t_cor_ps`
- top x-axis は raw `t_ps`

SRKR:

- `x` と `y` の2列
- selected signal mode に応じて X/Y または R/Theta
- top x-axis は uncorrected `x_um` / `y_um`

STRKR / SRKR 2D:

- 左列: current slow line の fast axis vs signal line plot 2枚
- 右列: x が fast axis、y が slow axis の heatmap 2枚
- signal mode は X/Y または R/Theta
- non-serpentine 固定で、各 line は毎回 fast min -> max
- heatmap は absolute max で正規化し、red/blue diverging lookup table で表示
- ETA は STRKR/SRKR 2D のみ。最初の fast line が終わり、次の slow move が終わってから推定を開始

## Lock-In Display

lock-in live status:

- Overload
- Sensitivity
- TC
- Frequency
- X/Y/R/Theta

各 wait row の `Use TC*4` は、connected lock-in の time constant から wait を入れます。連打中は device command が重ならないように無視されます。stale VISA handle の場合は `lockin.main` を一度 reconnect して retry します。

## Shutdown

window close 時は live polling を止め、active worker を待ち、`DeviceCommandWorker` 経由で `disconnect_all()` を main thread 外で実行します。
