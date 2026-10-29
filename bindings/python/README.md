# Rust版をNotebookから使う

GUI・測定・装置制御はRustで動作します。このクライアントはJSONで命令と結果を渡す窓口です。Pythonの装置ドライバーやQtは不要です。

## 互換性

設定JSONと`--stdio`の命令・イベントはrevision 1を維持します。
記録はrevision 3です。CSVの列は同じですが、X/Y/Rの固定7桁丸めを廃止し、
取得したf64値を復元できる指数表記に変更しました。`.run.json`と
`.raw.csv`をCSVと一緒に保管してください。装置応答の文字列を保持し、
不明な分解能・不確かさを推測しません。桁数は物理的な精度の保証ではありません。
`PROFILE_COMPATIBILITY_REVISION`と`JSON_LINES_API_REVISION`は`1`、
`RECORDING_COMPATIBILITY_REVISION`は`5`です。既存設定と旧CSVは引き続き読み取れます。
旧CSVで丸められた桁は復元できません。
0.3.6から固定軸の読取も保存し、U/Vの指令値と実読取を区別します。[座標の保存](../../docs/reference/measurement-integrity.md)を参照してください。

## 最初の準備

WindowsではTRKRのSetupを実行して`C:\\Program Files\\Kohdalab\\KohdaLab TRKR`へ導入し、Macでは **KohdaLab TRKR.app** を `/Applications` に配置してください。portable版はフォルダー全体を展開します。

ソースからクライアントを使う場合は、Notebookと同じPython環境へインストールします。

```sh
python -m pip install .
```

同梱の6つのNotebookは、同じフォルダー内のクライアント、隣接する `notebook-client/` またはソースの `bindings/python/` を自動で読み込みます。古いpip版があっても同梱版を優先します。

## パス指定なしで試す

```python
from kohdalab_trkr import Client, find_binary, find_profile, prepare_profile

print("Rust:", find_binary())
print("設定:", find_profile())
prepared = prepare_profile()  # Rustによる設定の読取・検証。装置には接続しない。
plan = dict(prepared["plans"]["trkr"])
plan["interval_s"] = 0.0  # シミュレーションを待ち時間なしで試す

with Client() as trkr:
    events = trkr.run(prepared["hardware"], plan, {"type": "demo"})

samples = [event["data"] for event in events if event["event"] == "sample"]
print(f"取得点数: {len(samples)}")
```

この例はシミュレーションです。装置への接続・移動・CSV保存は行いません。同梱の `trkr_notebook.ipynb` なども `HARDWARE = False` で始まります。

## 探索順

Rust本体は次の順に探します。探索自体はファイルの存在確認だけです。

1. `Client(binary=...)` などの明示指定、次に `TRKR_BINARY`
2. 作業フォルダーと近くのportable版・ソースビルド・Macアプリ
3. `PATH` 上の `kohdalab-trkr`
4. Windows: `C:\\Program Files\\Kohdalab\\KohdaLab TRKR`
5. Mac: `/Applications/KohdaLab TRKR.app`、次に `~/Applications/KohdaLab TRKR.app`

設定は明示指定／`TRKR_PROFILE`、またはユーザーの `~/.kohdalab/trkr/config.json` だけを使用します。サンプルや同梱の `default.json` を暗黙に測定設定として選ぶことはありません。

**使用する設定の表示を確認してください。** 特定の実験設定を使う場合は明示指定できます。

```python
prepared = prepare_profile("C:/Users/me/.kohdalab/trkr/kim.json")
# Mac/Linux: prepare_profile("~/.kohdalab/trkr/kim.json")
```

特殊な保存先では、Notebookの最初で指定します。指定先が存在しない場合はエラーになり、別のアプリや設定へ勝手に切り替えません。

```python
import os
os.environ["TRKR_BINARY"] = "D:/Lab/TRKR/kohdalab-trkr.exe"
os.environ["TRKR_PROFILE"] = "D:/Lab/config/kim.json"
```

## 旧Notebookからの移行

旧 `kohdalab.api.Experiment` と新クライアントは同じAPIではありません。既存の実験Notebookはコピーを作り、まず同梱の対応するNotebookでシミュレーションを確認してください。JSONはcanonicalな`instruments`／`measurements`形式が対象です。任意名の機器選択や旧Advanced形式など、未互換の設定も残っています。[互換性監査](https://github.com/woolen-yarn/kohdalab-trkr/blob/v0.3.4/docs/trkr_portable_rust.md)に残件を記載しています。測定CSVは専用Demoで読めますが、旧APIや保存形式全体の厳密な同一性を保証するものではありません。

| 旧Notebookの役割 | Rust版での置き換え |
| --- | --- |
| `load_config()`、Pythonでの計画作成 | `prepare_profile()` → `hardware` と `plans` |
| `Experiment(config, auto_connect=False)` | `Client()`（起動だけでは装置へ接続しない） |
| `experiment.run_trkr(...)` などの測定実行 | `client.run(hardware, plan, source, output)` |
| ライブ更新・測定行の取得 | `sample` イベントの `data` を利用 |
| `point["R_V"]` などの信号参照 | `sample["signal"]["r_v"]`（X/Y/Thetaも同様） |
| 測定の中止 | 別スレッド／セルから `client.stop()` |

Signal Monitor、TRKR、SRKR、STRKR、SRKR 2Dは、`prepared["plans"]` の `signal_monitor`、`trkr`、`srkr`、`strkr`、`srkr_2d` を選びます。小さい範囲への変更や、バックグラウンド測定・Stop・プロットの例は各Notebookにあります。

`sample["target"]["corrected"]` は指令座標、`sample["position"]["physical"]` は読取位置です。両者を混同せず、従来形式のCSVへの変換・保存はRust側へ任せます。Notebook内の `PLOT_RESULT = True` を使う場合だけ、同じPython環境へ `matplotlib` を追加してください。

CSVを保存する場合は `run(..., output="new_run.csv")` と指定します。同名ファイルは上書きしません。実機で使う際は、Notebookの表示した設定と計画を確認した上で `HARDWARE` を明示的に変更します。原点復帰は自動で行いません。

`run()` は同期処理です。並列の測定ジョブは同じクライアント内で直列化します。生の移動命令は `move(...)` の後に `wait_for_finished()` で完了を待ちます。

## 開いているGUIをPythonから制御する / External control of the open GUI

通常のGUIもローカルAPIを公開します。すでに開いているGUIには、そのまま接続できます。Pythonから新規に起動する場合だけ`GuiClient.launch()`を使います。通常のGUI起動・表示・設定は変わりません。

```python
# 必要なときだけ、設定を指定して新しいGUIを起動する。
gui = GuiClient.launch(profile="C:/Lab/kim.json")
```

`GuiClient`は機器へ直接接続せず、同じPCのGUIを探して操作します。`with`に入るとGUIの外部制御権を取得し、1秒ごとに更新します。Pythonが権限を持つ間はGUIが操作を受け付けず、GUIにはPython制御中であることが表示されます。GUIの**Take control**はPythonの権限を取り消します。その後のPythonの変更要求は再取得せずエラーになります。複数のGUIが開いている場合は、誤った装置を選ばないようエラーになります。その場合はセッションJSONを`session=`で明示してください。

```python
from kohdalab_trkr import GuiClient

with GuiClient() as gui:
    # 開いているGUIの設定を読み、同じ設定をGUI側に反映して測定する。
    selected = gui.plan("trkr")
    gui.set_plan({**selected["plan"], "interval_s": 2.0})
    gui.set_output(directory="D:/TRKR/data", filename="trkr_field_0.50T.csv")

    # 例: 外部の磁場・温度コントローラーをここで変更して安定を待つ。
    external_field.set_field(0.50)
    external_field.wait_stable()

    run = gui.start("trkr")
    record = gui.wait(run["id"], timeout=3600)
    print(record["output"])
```

外部条件は出力ファイル名など実験側の命名規則で管理してください。明示した相対パスはPythonの作業フォルダー基準です。`output`を省略するとGUIの保存先・ファイル名・Auto設定を使います。`set_output(filename="field_0.5T.csv")`のように`.csv`付きで指定でき、GUIが通常の保存名へ正規化します。`config()`、`plan()`、`output()`は現在のGUI状態を読むだけです。`settings()`はGUIのロックイン接続で設定を取得するジョブです。`load_config()`、`configure()`、`set_plan()`、`set_output()`、`move()`、`initialize()`、`connect()`、`disconnect()`、`set_lockin()`、`auto_lockin()`は、取得済みの制御権でGUIに命令します。すべての機器設定、補償移動、接続、保存はGUI側が実行します。

空間位置を変えてTRKRを繰り返す場合も、直接シリアルポートを開かずGUIに順番に任せます。

```python
with GuiClient.launch(profile="C:/Lab/kim.json") as gui:
    gui.wait(gui.connect()["id"])  # 設定内の機器へGUI側で接続
    gui.set_output(directory="D:/TRKR/data", auto_timestamp_suffix=False, workflow="trkr")
    for x_um in [0.0, 5.0, 10.0]:
        gui.wait(gui.move([None, x_um, None])["id"])  # 測定座標のx、µm
        gui.set_output(filename=f"trkr_x_{x_um:g}um.csv", workflow="trkr")
        gui.run("trkr")
```

`move()`の順序は`[t (ps), x (µm), y (µm)]`、`None`は動かさない軸です。既定の`measurement`座標はGUIの共有原点を使います。機器設定のJSONを置き換えるときとDemo／実機を切り替えるときは、先に`disconnect()`の完了を待ちます。原点復帰は自動で行わず、`initialize(home=True)`を明示した場合だけ要求します。Pythonが異常終了した場合は約6秒で操作権が失効し、動作中ならその完了までロックを維持します。GUIのStop／Take controlは常に使えます。

For a spatial-position loop, use `move([None, x_um, None])`, wait for its receipt, then run TRKR as above. With the default measurement coordinates, targets use the GUI’s shared zero. `[t, x, y]` units are ps/µm/µm; `None` leaves an axis unchanged. Disconnect and wait before replacing the hardware JSON or changing Demo/hardware source. Homing is never implicit: request `initialize(home=True)` explicitly. By default, if Python crashes its lease expires after about six seconds; an active operation keeps the GUI locked until completion, with Stop/Take control available throughout. With `stop_on_disconnect=True`, lease expiry instead requests Stop and keeps the lock through cleanup.

結果の受付記録はGUIの起動中に直近32件を保持します。CSVは従来どおり保存されます。通信エラー時は`status()`で状態を確認してから判断してください。`start(request_id=...)`にUUIDを明示し、同じIDを再送した場合は再測定せず元の受付記録を返します。

既定ではPythonクライアントを閉じても測定は停止せず、GUIも閉じません。手動で停止するときは、`gui.stop(run["id"])`またはGUIのStopを使います。`wait()`はGUI Stopによる中断を`GuiRunStopped`として通知し、既定ではタイムアウトしてもStopを要求せず、再開始は常に行いません。新規GUIは`GuiClient.launch(profile="C:/Lab/kim.json")`で起動できます。Python終了後もGUIは独立して動作します。

The ordinary GUI publishes its local API, so `GuiClient()` attaches to its current settings without a special launch flag. `GuiClient.launch(profile=...)` starts a new independent GUI when needed. The client never talks to an instrument directly. Entering its context acquires a six-second GUI lease and refreshes it every second: while held, the GUI displays that Python controls it and rejects competing edits. **Take control** in the GUI revokes that lease; later Python mutations fail rather than silently reclaiming it. `config()`, `plan()`, and `output()` are read-only; `settings()` reads lock-in settings as a GUI job. Profile, plan, output, device, motion, and lock-in changes execute in the GUI after the lease is held. By default, closing Python releases its lease and lets the run continue. Selected stop conditions can request Stop; closing Python never closes the GUI. Multiple live GUIs are rejected instead of selecting an arbitrary instrument; pass the session JSON as `session=` in that case.

Use the output file name or your experiment naming convention for external conditions. Explicit relative output paths are resolved against Python’s working directory; omit `output` to use the GUI directory, name, and Auto setting. `set_output(filename="field_0.5T.csv")` is accepted; the GUI normalizes the CSV suffix for its normal output naming. `plan()` changes only the measurement plan. Connected hardware, Demo/GPIB source selection, and current device settings remain those of the GUI. The GUI retains the latest 32 run receipts during its lifetime; CSV storage is unchanged. After a connection error, check `status()` before deciding what to do. An explicit UUID passed to `start(request_id=...)` makes resubmission return the original receipt instead of starting another measurement. By default closing a Python client lets the measurement continue; use `gui.stop(run["id"])` or the GUI Stop button deliberately. `wait()` raises `GuiRunStopped` when GUI Stop interrupts the run, and its timeout requests Stop only with `stop_on_timeout=True`. No policy restarts a run.


### Stop conditions / 停止条件

The flags are independent booleans accepted by `GuiClient(...)` and `GuiClient.launch(...)`. All default to **False** in TRKR. Device errors still follow the existing engine protection and cleanup, regardless of these Python policies. Policies are scoped to the owning client; read-only observers cannot stop someone else's run. Normal acquisition, file formats, and GUI controls are unchanged.

各フラグは`GuiClient(...)`と`GuiClient.launch(...)`で個別に指定でき、TRKRではすべて**False（継続）**が既定です。装置異常時の既存の保護・終了処理はフラグによらず実行します。停止条件は制御権を持つクライアントに適用され、状態を読むだけのクライアントが他の測定を止めることはありません。

| Option / 設定 | Trigger / 条件 | Behavior / 動作 |
| --- | --- | --- |
| `stop_on_disconnect` | Client release/close or GUI detects heartbeat expiry (about 6 s) / 解放・終了、またはGUI側でheartbeat失効を検出（約6秒） | GUI requests Stop for the active owned job / 実行中の対象処理にStopを要求 |
| `stop_on_error` | Python exception escapes the `with` block / `with`ブロックからPython例外が抜ける | Stop requested on release; original Python exception preserved / 解放時にStopを要求し、元のPython例外を保持 |
| `stop_on_timeout` | `wait()` or `run()` reaches its wait deadline / `wait()`・`run()`の待機時間切れ | Requests Stop once, then raises `GuiTimeoutError` / Stopを1回要求して`GuiTimeoutError`を送出 |

```python
from kohdalab_trkr import GuiClient, GuiTimeoutError

with GuiClient(stop_on_disconnect=True, stop_on_error=True,
               stop_on_timeout=False) as gui:
    gui.set_output(directory="D:/TRKR/data", filename="field_0.5T")
    run = gui.start("trkr")
    try:
        result = gui.wait(run["id"], timeout=600)
    except GuiTimeoutError:
        result = gui.stop_and_wait(run["id"], timeout=40)
```

`stop()` acknowledges a request. `stop_and_wait(run_id, timeout=40, poll_interval=0.1)` returns a **completed or stopped receipt only after worker cleanup**; an already terminal job is not stopped again. `wait()` raises `GuiRunStopped` on a stopped job. Both expose failures through `GuiError` subclasses with `.code` and `.receipt`. `GuiTimeoutError` also inherits `TimeoutError`; its `.receipt` is the last observed state, and `.stop_error` contains a failed automatic Stop request, if any. A timeout or a lost transport **does not confirm stationary hardware**. Inspect the GUI/instruments; there is no automatic retry, reacquisition, homing, or restart. Stop is the ordinary existing controller Stop, not an instantaneous hardware emergency stop. A hung GUI or failed instrument connection can prevent confirmation.

`stop()`は停止要求の受付を返します。`stop_and_wait()`はワーカーの終了処理後にcompleted/stoppedの結果を返し、終了済み処理にはStopを再送しません。`wait()`で中断を検出すると`GuiRunStopped`です。例外の`.code`は機械判読用コード、`.receipt`は最後の処理結果です。`GuiTimeoutError`は`TimeoutError`の派生でもあり、自動Stop要求に失敗した場合は`.stop_error`に原因を保持します。**タイムアウトや通信断は装置の停止確認を意味しません。** 自動再試行・権限再取得・原点復帰・測定再開はしません。Stopは既存の通常停止処理であり、GUI停止や装置通信断では完了確認できない場合があります。`stop_on_error`は`with`外で発生した任意のPython例外を監視するものではありません。複数条件が重なれば、いずれかの有効な条件で停止します。

| Exception / 例外 | Code / コード |
| --- | --- |
| `GuiTransportError` | `transport`, `protocol` |
| `GuiOwnershipError` | `ownership_lost` |
| `GuiDeviceError` | `device_error` |
| `GuiProtectionError` | `protection` |
| `GuiSaveError` | `save_failed` |
| `GuiInitializationError` | `initialization_failed` |
| `GuiRunStopped` | `stopped` |
| `GuiTimeoutError` | `timeout` |
| `GuiError` (also `RuntimeError`) | Other/unknown codes, including `job_failed` / その他・未分類 |

Classification uses explicit codes, never text matching. Older GUIs without codes remain generic errors; no error is guessed to be a protection failure. Both IV and TRKR provide the same option/exception names and `stop_and_wait` terminal semantics. **Defaults differ intentionally:** TRKR continues by default; IV retains its existing Stop + Output OFF protection. IV release/close still turns outputs off even when its disconnect policy is disabled. See the IV Python README for its voltage-source safety details.

分類は明示的なコードによって行い、エラー文面から推測しません。コードを持たない旧GUIの例外は汎用エラーです。IVも同じ設定名・例外名・停止完了待ちを提供しますが、**既定はTRKR＝継続、IV＝既存のStop＋Output OFF**です。IVの正常な解放・終了時のOutput OFFは、通信失効時ポリシーとは独立して維持します。

## English

The GUI, acquisition, and device control run in Rust. This client passes commands and results over JSON; Python device drivers and Qt are not required.

### Compatibility

Profiles and `--stdio` commands/events remain revision 1. Recording revision 3
keeps the CSV columns but removes fixed seven-digit X/Y/R rounding. Scientific
text round-trips the acquired f64 value. Keep `.run.json` and `.raw.csv`
with the CSV: raw instrument response text is retained; unknown resolution and
uncertainty remain unknown. Digit count does not certify physical accuracy.
`PROFILE_COMPATIBILITY_REVISION` and `JSON_LINES_API_REVISION` remain `1`;
`RECORDING_COMPATIBILITY_REVISION` is `5`. Existing profiles and old CSV files
remain readable. Digits already rounded out of old CSV files cannot be recovered.
From 0.3.6, available fixed-axis readings are saved and commanded U/V is separated from measured U/V. See [coordinate recording](../../docs/reference/measurement-integrity.md).

### Setup

On Windows, run TRKR Setup to install at `C:\\Program Files\\Kohdalab\\KohdaLab TRKR`. On macOS, place **KohdaLab TRKR.app** in `/Applications`. Extract the complete portable folder. To use the source client, install it into the Notebook's Python environment using `python -m pip install .`.

The six bundled notebooks load their adjacent client, `notebook-client/`, or the source `bindings/python/` automatically, preferring the bundled client over an older pip installation.

### Try without specifying paths

Use the `Client`, `find_binary`, `find_profile`, and `prepare_profile` example above. `prepare_profile()` reads and validates the profile in Rust without connecting instruments. Copy `prepared["plans"]["trkr"]`, set `interval_s = 0.0` for fast simulation, and run with the source `{"type": "demo"}`. Extract `sample` events to count acquired points.

This example is simulation only: it does not connect instruments, move hardware, or save CSV. Bundled notebooks such as `trkr_notebook.ipynb` also start with `HARDWARE = False`.

### Discovery order

Executable discovery only checks whether files exist, in this order:

1. An explicit argument such as `Client(binary=...)`, then `TRKR_BINARY`.
2. The working directory and nearby portable distributions, source builds, or Mac apps.
3. `kohdalab-trkr` on `PATH`.
4. Windows: `C:\\Program Files\\Kohdalab\\KohdaLab TRKR`.
5. macOS: `/Applications/KohdaLab TRKR.app`, then `~/Applications/KohdaLab TRKR.app`.

Profile discovery uses only an explicit argument or `TRKR_PROFILE`, then the dedicated user profile `~/.kohdalab/trkr/config.json`.

**Check the displayed selected profile.** To choose an experiment explicitly, use `prepare_profile("C:/Users/me/.kohdalab/trkr/kim.json")`, or `prepare_profile("~/.kohdalab/trkr/kim.json")` on Mac/Linux.

For custom locations, set `TRKR_BINARY` and `TRKR_PROFILE` at the start of the Notebook, as in the example above. A missing explicit path is an error; the client does not silently switch to another application or profile.

### Migrating old notebooks

The old `kohdalab.api.Experiment` and the new client are different APIs. Copy an existing experimental notebook and first verify simulation with the corresponding bundled notebook. Supported JSON uses canonical `instruments` / `measurements`; arbitrary named-device selection and old Advanced profiles still have compatibility gaps. See the [compatibility audit](https://github.com/woolen-yarn/kohdalab-trkr/blob/v0.3.4/docs/trkr_portable_rust.md). The dedicated Demo can read measurement CSVs; this does not guarantee exact equivalence of the old API or every saved format.

| Old Notebook role | Rust-backed replacement |
| --- | --- |
| `load_config()` and Python planning | `prepare_profile()` → `hardware` and `plans` |
| `Experiment(config, auto_connect=False)` | `Client()`; starting the client alone does not connect hardware |
| `experiment.run_trkr(...)` and other runs | `client.run(hardware, plan, source, output)` |
| Live updates and measurement rows | Use the `data` of `sample` events |
| `point["R_V"]` and other signals | `sample["signal"]["r_v"]`; analogous X/Y/Theta fields |
| Stop acquisition | `client.stop()` from another thread/cell |

Select `signal_monitor`, `trkr`, `srkr`, `strkr`, or `srkr_2d` in `prepared["plans"]` for the corresponding workflow. Each notebook shows smaller ranges, background acquisition, Stop, and plotting.

`sample["target"]["corrected"]` is the commanded coordinate; `sample["position"]["physical"]` is readback. Keep them distinct and let Rust convert/save the established CSV format. Add `matplotlib` to the same Python environment only when using `PLOT_RESULT = True`.

To save CSV, specify `run(..., output="new_run.csv")`. Existing files are not overwritten. Before hardware use, review the displayed profile and plan, then explicitly change `HARDWARE`. Homing is not automatic.

`run()` is synchronous, and concurrent acquisition jobs within one client are serialized. After a raw `move(...)` command, use `wait_for_finished()` to wait for completion.

## 0.4 CSV migration / CSV移行

New runs use analysis main CSV + raw CSV + run JSON. Main signal names are `X`, `Y`, `R` with `signal_unit`; do not assume V when the unit is unknown. `elapsed_s` replaces duplicate `acquisition_elapsed_s`. Controller mm/degree/pulse readings move to raw CSV. Old CSV and old JSONL sidecars are still readable/copyable; existing files are never rewritten. The JSON-lines Notebook signal API is unchanged.

新規測定はmain CSV・raw CSV・run JSONの3ファイルです。信号列は`X`・`Y`・`R`と`signal_unit`になり、単位不明時にVと仮定しません。経過秒は`elapsed_s`に統一し、コントローラー単位はrawに移動します。旧CSVの読込・旧JSONL付きデータのコピーと、NotebookのJSON-lines信号APIは維持します。既存ファイルは書き換えません。
