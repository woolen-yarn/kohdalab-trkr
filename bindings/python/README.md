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

WindowsではTRKRのSetupを実行、Macでは **KohdaLab TRKR.app** を `/Applications` または `~/Applications` に配置してください。portable版はフォルダー全体を展開します。

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
4. Windows: インストーラーが記録したユーザーの保存先、次に `%LOCALAPPDATA%\KohdaLab\TRKR`
5. Mac: `~/Applications/KohdaLab TRKR.app`、次に `/Applications/KohdaLab TRKR.app`

設定は明示指定／`TRKR_PROFILE`、近くの `config/default.json`、ユーザーの `~/.kohdalab/config/trkr-next.json`・`default.json`、ソースの既定設定、検出したアプリの同梱設定の順に探します。`trkr-next.json` はRust GUIが保存する既存の設定名です。

**使用する設定の表示を確認してください。** portable版の近くに設定がある場合は、その設定を優先します。ソースの既定設定より、GUIで保存したユーザー設定を優先します。特定の実験設定を使う場合は明示指定できます。

```python
prepared = prepare_profile("C:/Users/me/.kohdalab/config/kim.json")
# Mac/Linux: prepare_profile("~/.kohdalab/config/kim.json")
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

外部制御を使う場合だけ、専用引数でGUIを起動します。通常のGUI起動・表示・設定は変わりません。

```powershell
# Windows
& "$env:LOCALAPPDATA\KohdaLab\TRKR\kohdalab-trkr.exe" --gui-api
```

```sh
# macOS
'/Applications/KohdaLab TRKR.app/Contents/MacOS/kohdalab-trkr' --gui-api
```

`GuiClient`はGUIを起動せず、機器へ直接接続せず、同じPCで`--gui-api`により起動済みのGUIを探して測定を開始・完了待ち・Stopできます。開始した計画はGUIの通常の入力欄・進捗・プロットに反映されます。複数のGUIが開いている場合は、誤った装置を選ばないようエラーになります。その場合はセッションJSONを`session=`で明示してください。

```python
from kohdalab_trkr import GuiClient

with GuiClient() as gui:
    # GUIで現在選ばれているTRKR計画を読み、必要ならPythonで計画だけ変更する。
    selected = gui.plan("trkr")
    plan = {**selected["plan"], "interval_s": 2.0}

    # 例: 外部の磁場・温度コントローラーをここで変更して安定を待つ。
    external_field.set_field(0.50)
    external_field.wait_stable()

    run = gui.start("trkr", plan=plan, output="trkr_field_0.50T.csv")
    record = gui.wait(run["id"], timeout=3600)
    print(record["output"])
```

外部条件は出力ファイル名など実験側の命名規則で管理してください。明示した相対パスはPythonの作業フォルダー基準です。`output`を省略するとGUIの保存先・ファイル名・Auto設定を使います。`plan()`で変えるのは測定計画だけで、ハードウェア設定・GUIで接続済みの測定源・Demo/GPIB選択・機器設定はGUIの現在のものを使います。

結果の受付記録はGUIの起動中に直近32件を保持します。CSVは従来どおり保存されます。通信エラー時は`status()`で状態を確認してから判断してください。`start(request_id=...)`にUUIDを明示し、同じIDを再送した場合は再測定せず元の受付記録を返します。

Pythonクライアントを閉じても測定は停止しません。停止が必要なときだけ、`gui.stop(run["id"])`またはGUIのStopを使います。`wait()`はGUI Stopによる中断を`GuiRunStopped`として通知し、タイムアウトしてもStopや再開始をしません。

Start the GUI with `--gui-api` only when external control is required: on Windows use `& "$env:LOCALAPPDATA\KohdaLab\TRKR\kohdalab-trkr.exe" --gui-api`; on macOS use `'/Applications/KohdaLab TRKR.app/Contents/MacOS/kohdalab-trkr' --gui-api`. Ordinary GUI startup, controls, and settings remain unchanged. `GuiClient` neither launches the GUI nor connects to instruments: it finds one GUI already started with this option on the same computer and can start, wait for, or stop its measurement. A remotely started plan updates the normal GUI fields, progress, and plots. Multiple live GUIs are rejected instead of selecting an arbitrary instrument; pass the session JSON as `session=` in that case.

Use the output file name or your experiment naming convention for external conditions. Explicit relative output paths are resolved against Python’s working directory; omit `output` to use the GUI directory, name, and Auto setting. `plan()` changes only the measurement plan. Connected hardware, Demo/GPIB source selection, and current device settings remain those of the GUI. The GUI retains the latest 32 run receipts during its lifetime; CSV storage is unchanged. After a connection error, check `status()` before deciding what to do. An explicit UUID passed to `start(request_id=...)` makes resubmission return the original receipt instead of starting another measurement. Closing a Python client never cancels a measurement; use `gui.stop(run["id"])` or the GUI Stop button deliberately. `wait()` raises `GuiRunStopped` when GUI Stop interrupts the run, and its timeout never stops or restarts a run.

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

On Windows, run TRKR Setup. On macOS, place **KohdaLab TRKR.app** in `/Applications` or `~/Applications`. Extract the complete portable folder. To use the source client, install it into the Notebook's Python environment using `python -m pip install .`.

The six bundled notebooks load their adjacent client, `notebook-client/`, or the source `bindings/python/` automatically, preferring the bundled client over an older pip installation.

### Try without specifying paths

Use the `Client`, `find_binary`, `find_profile`, and `prepare_profile` example above. `prepare_profile()` reads and validates the profile in Rust without connecting instruments. Copy `prepared["plans"]["trkr"]`, set `interval_s = 0.0` for fast simulation, and run with the source `{"type": "demo"}`. Extract `sample` events to count acquired points.

This example is simulation only: it does not connect instruments, move hardware, or save CSV. Bundled notebooks such as `trkr_notebook.ipynb` also start with `HARDWARE = False`.

### Discovery order

Executable discovery only checks whether files exist, in this order:

1. An explicit argument such as `Client(binary=...)`, then `TRKR_BINARY`.
2. The working directory and nearby portable distributions, source builds, or Mac apps.
3. `kohdalab-trkr` on `PATH`.
4. Windows: the per-user location recorded by Setup, then `%LOCALAPPDATA%\KohdaLab\TRKR`.
5. macOS: `~/Applications/KohdaLab TRKR.app`, then `/Applications/KohdaLab TRKR.app`.

Profile discovery uses an explicit argument or `TRKR_PROFILE`, nearby `config/default.json`, user `~/.kohdalab/config/trkr-next.json` or `default.json`, source defaults, then bundled defaults from the discovered app. `trkr-next.json` remains the Rust GUI's existing settings filename.

**Check the displayed selected profile.** A nearby portable profile wins, and GUI-saved user settings win over source defaults. To choose an experiment explicitly, use `prepare_profile("C:/Users/me/.kohdalab/config/kim.json")`, or `prepare_profile("~/.kohdalab/config/kim.json")` on Mac/Linux.

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
