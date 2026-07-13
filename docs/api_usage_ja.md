# KohdaLab API 使用メモ

English version: [`api_usage.md`](api_usage.md).

この文書は notebook、script、CLI、TRKR GUI が使う現在の public API の実用メモです。

## Entry Point

基本の入口は `kohdalab.api.Experiment` です。

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

config = load_config("config/kikuchi.json")
experiment = Experiment(config)

plan = trkr_plan_from_config(config)
rows = experiment.run_trkr(plan=plan)
```

`Experiment` は 1つの長生きする `DeviceSession` を持ちます。device read、move、measurement run はすべてこの session を通ります。

GUI は明示接続モードで使います。

```python
experiment = Experiment(config, auto_connect=False)
```

`auto_connect=False` では、未接続 device に対する read/move は `Device not connected: ...` を出します。`connect_device()` と `connect_all()` はどちらの mode でも明示接続として使えます。

## Required Devices

必要 device の判定は API 側にあります。

```python
experiment.required_devices("trkr")
experiment.required_devices("srkr", axis="x")
experiment.required_devices("strkr", fast_axis="t", slow_axis="x")
experiment.required_devices("srkr_2d", fast_axis="x", slow_axis="y")

missing = experiment.missing_devices("trkr")
```

既定の必要接続:

```text
Signal Monitor: lockin.main
TRKR:           lockin.main, delay_stage.t
SRKR:           lockin.main, active scanner axis
STRKR:          lockin.main, delay_stage.t, active spatial scanner axis
SRKR 2D:        lockin.main, scanner.x, scanner.y
```

## Scan Plans

TRKR/SRKR/STRKR/SRKR 2D は scan plan を使います。plan は operator に見える corrected target と、実際に move する absolute point を対応づけます。

```python
from kohdalab.api import (
    srkr_2d_plan_from_config,
    srkr_plan_from_config,
    strkr_plan_from_config,
    trkr_plan_from_config,
)

trkr = trkr_plan_from_config(config)
srkr_x = srkr_plan_from_config(config, axis="x")
strkr = strkr_plan_from_config(config, fast_axis="t", slow_axis="x")
srkr_2d = srkr_2d_plan_from_config(config, fast_axis="x", slow_axis="y")
```

STRKR は `t` と `x/y` の時空間 2D scan です。fast/slow の組み合わせは `(t,x)`, `(t,y)`, `(x,t)`, `(y,t)` です。SRKR 2D は `x/y` の空間 2D scan で、組み合わせは `(x,y)` または `(y,x)` です。

2D plan は corrected coordinate を使います。scan しない軸は触らず、operator が事前に move した位置のままです。`Scan2DPlan` には `fast_point_count`, `slow_point_count`, `total_points` があります。

## Running Measurements

```python
rows = experiment.run_signal_monitor(plan=signal_monitor_plan_from_config(config))
rows = experiment.run_trkr(plan=trkr_plan_from_config(config))
rows = experiment.run_srkr(plan=srkr_plan_from_config(config, axis="x"))
rows = experiment.run_strkr(plan=strkr_plan_from_config(config, fast_axis="t", slow_axis="x"))
rows = experiment.run_srkr_2d(plan=srkr_2d_plan_from_config(config, fast_axis="x", slow_axis="y"))
```

`Experiment.run_*()` は既存の `experiment.session` を再利用し、run 後も接続を維持します。standalone の `run_trkr(config, ...)` などは、session が渡されない場合だけ一時 session を作り、最後に disconnect します。

決定的に解放する場合は`Experiment`または`DeviceSession`をcontext managerとして使います。`close()`は冪等で`disconnect_all()`と同じです。

```python
with Experiment(config) as experiment:
    experiment.connect_all()
    rows = experiment.run_trkr()
```

処理本体で例外が発生しても終了時に切断します。本体とcleanupが両方失敗した場合は本体例外を維持し、cleanup失敗をexception noteへ追加します。

`connect_all()`はtransactionalです。後続deviceの接続に失敗した場合、その呼び出し中に取得した所有権だけを逆順で全て解放し、呼び出し前から存在した接続は維持します。rollback失敗は元の接続例外を維持したままexception noteへ追加します。

initializeもhome/origin移動前に接続所有権を登録し、同じhandleをservice層へ渡します。初期化失敗時はその呼び出しで新規取得した所有権だけを解放し、既存接続は維持します。`initialize_xy()`も2軸全体で同じ規則を適用します。エラー前に完了した物理移動そのものは巻き戻せません。

`connected_devices()`はsession辞書への登録だけでなく各wrapperの`is_connected()`を確認します。`False`または状態確認例外は未接続として扱います。stale handleでのdevice操作はdisconnect/reconnectを求める明示エラーにし、stale所有権を解放するdisconnectは引き続き実行できます。

同じcached handleを共有するsessionは、そのhandleのreentrant I/O lockも共有します。同一handleへのsession-level操作、connect、disconnectはsessionをまたいで直列化し、無関係なhardwareへの操作は並行実行できます。

device 接続中でも measurement/output 設定は `experiment.config` で更新できますが、接続中 instrument の resource、port、controller、actuator 定義の変更・削除は拒否されます。先に disconnect してください。読み取り専用の status 操作は measurement/output 設定の更新を妨げません。同一instrumentへのI/Oはdevice別lockで直列化し、無関係なinstrumentは並行動作できます。移動、初期化、測定中のconfig変更は禁止を維持します。`disconnect_all()` は1台の close が失敗しても全deviceの切断を試行し、最後に失敗したreferenceをまとめて報告します。複数の`DeviceSession`が同じcached hardware handleを共有する場合はsessionごとに所有権を持ち、最後の所有者がdisconnectした時点でのみ実接続を閉じます。同一session内の`connect_device()`再呼び出しは冪等です。同じ物理接続を共有する全sessionは完全に同じinstrument configを使う必要があり、不一致は異なるtop-level fieldを示してdevice I/O前に拒否します。最後の所有権解放後は別設定で接続できます。各sessionも接続時のdevice configを固定保存します。nested instrument configの直接変更は後続device操作前に検出され、disconnectでは固定したresource/portを使うため元の接続を取り残しません。

delay-stage measurement coordinate には固定physical zeroが必要です。既知stage profileは設定済みtravelのmin/max中央を使い、最大limitがないcustom stageはfiniteな`zero_pos_mm`を必須とします。現在位置から移動するzeroは推定しません。instrument coordinateはinteger pulseのみ許可し、小数pulseと非有限targetはcontroller command前に拒否します。

scanner変換と初期化のorigin優先順位は、finiteな明示`origin_pos`、finiteな`min_pos`/`max_pos`中央、最後にzeroです。明示originは設定limit内である必要があります。scanner target、hardware reading、scale/origin、software hysteresis distanceはfinite必須で、hysteresis enabledはboolean、directionは対応するpositive/negative aliasのみ許可します。

config loadはlock-in model、controller、stage、actuator名をcanonical化し、同梱driver/catalogと照合します。stage/controllerとactuator/controllerの互換性もsession生成前に検証します。同一cached handleを別session lockから操作しないよう、重複lock-in resource、delay-stage controller/port、scanner controller/port/axisを拒否します。measurement固有device keyも既存instrument entryへ解決できる必要があります。

measurement timestamp は `Z` suffix 付き RFC 3339 UTC です。各 measurement CSV には `<name>.csv.meta.json` が併設され、run ID、redacted config snapshot と SHA-256、software version、予定／保存 point 数、terminal status (`completed`, `stopped`, `failed`, `interrupted`)、CSV SHA-256 を記録します。手動 export で sidecar も同期させる場合は `write_measurement_rows()` を使います。

measurement runはoutput CSVをexclusive createします。CSVまたはsidecarが既に存在する場合、point iterationとdevice I/Oの前に失敗し、既存dataを暗黙にtruncateしません。auto filename suffixはmicrosecondを含みます。`write_measurement_rows()`も既定では置換を拒否し、GUI **Save Now**のような明示置換だけ`overwrite=True`を使います。この経路はtemporary CSVをwrite/fsyncしてからdestinationをatomic replaceし、hashが一致するsidecarを再生成します。

progress や live plot には callback を渡します。

```python
from kohdalab.api import format_point, make_trkr_live_update

live_update = make_trkr_live_update(y_key="R_V")

def on_point(point):
    print(format_point(point, axis_key="t_cor_ps"))
    live_update(point)

rows = experiment.run_trkr(plan=trkr_plan_from_config(config), on_point=on_point)
```

## Moving Devices

```python
position = experiment.move_delay_stage(0.0, coordinate="measurement")
position = experiment.move_scanner("x", 10.0, coordinate="measurement")
```

move は `on_status` と `on_position` callback を受け取れます。status 文字列は `kohdalab.api.status` に集約されています。

```python
from kohdalab.api import STATUS_MOVING_DELAY_STAGE, moving_axis_from_status

def on_status(status):
    axis = moving_axis_from_status(status)
    print(status, axis)

experiment.move_delay_stage(10.0, coordinate="measurement", on_status=on_status)

assert STATUS_MOVING_DELAY_STAGE == "moving delay stage"
```

measurement 側も同じ status channel を使います。代表値は `STATUS_RUNNING`, `STATUS_WAITING`, `STATUS_READING_LOCKIN`, `STATUS_SLOW_AXIS_READY`, `STATUS_STOPPED` です。

## Lock-In Settings

GUI は lock-in settings を現状 read/display のみにしていますが、API からは書き込みできます。

```python
applied = experiment.set_lockin_settings(
    "lockin.main",
    sensitivity=100e-6,
    time_constant=1.0,
    coupling="AC",
    slope=24,
)
```

指定した値だけを書き込みます。非対応 model-specific write は underlying driver の error として上がります。

## Row Schemas

row は plain dictionary ですが、field order は `kohdalab.api.measurement_rows` に集約されています。

```text
timestamp, measurement, fast_axis, slow_axis,
target_elapsed_s, target_t_cor_ps, target_x_cor_um, target_y_cor_um,
elapsed_s, t_cor_ps, t_ps, x_cor_um, x_um, y_cor_um, y_um,
X_V, Y_V, R_V, Theta_deg,
coordinate, delay_stage_mm, delay_stage_pulse,
x_scanner_mm, x_scanner_deg, y_scanner_mm, y_scanner_deg
```

Signal Monitor は `fast_axis=elapsed_s` と `target_elapsed_s` を使います。TRKR は `fast_axis=t` と `target_t_cor_ps`、SRKR は `fast_axis=x/y` と対応する `target_x_cor_um` / `target_y_cor_um` を使います。STRKR と SRKR 2D は `slow_axis` も持ち、scan する2軸の target を保存します。

`scan_axis` と generic な `target` は出しません。

## CLI

```powershell
kohdalab-cli --config config\kikuchi.json signal-monitor
kohdalab-cli --config config\kikuchi.json trkr
kohdalab-cli --config config\kikuchi.json srkr --axis x
kohdalab-cli --config config\kikuchi.json strkr --fast-axis t --slow-axis x
kohdalab-cli --config config\kikuchi.json srkr-2d --fast-axis x --slow-axis y
```

CLI は start/status/point progress を表示し、各 measurement の output 設定に従って CSV を書きます。最終的な `Saved` は device cleanup 成功後だけ表示します。終了コードは、完了が `0`、実行時または cleanup 失敗が `1`、引数/config 不正が `2`、キーボード割り込みが `130` です。

## Notebooks

maintained notebook は次の6本です。

```text
notebook/move_abs_notebook.ipynb
notebook/signal_monitor_notebook.ipynb
notebook/trkr_notebook.ipynb
notebook/srkr_notebook.ipynb
notebook/strkr_notebook.ipynb
notebook/srkr_2d_notebook.ipynb
```

これらは `Experiment`、scan-plan builder、`format_point()`、notebook live plot helper を使います。maintained notebook は `auto_connect=False` を明示しているため、config を確認してから `experiment.connect_all()` を実行します。

## GUI

```powershell
uv run kohdalab-gui
```

GUI は `Experiment(..., auto_connect=False)` を使います。Start 時に `Experiment.missing_devices(...)` を確認し、必要 handle が明示接続されるまで run しません。hardware-facing work は Qt worker 経由で実行され、main thread は widget 更新と signal handling に集中します。
