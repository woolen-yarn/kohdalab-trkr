# Rust版をNotebookから使う

GUI・測定・装置制御はRustで動作します。このクライアントはJSONで命令と結果を渡す窓口です。Pythonの装置ドライバーやQtは不要です。

## 互換性

設定JSON、`--stdio` のJSON-lines命令・イベント、測定CSV、隣接する
`.run.json` は、既存のフィールドを追加しない unversioned revision 1 として
管理されています。任意のユーザーメタデータはschema versionとして解釈しません。
Python側では `PROFILE_COMPATIBILITY_REVISION`、`JSON_LINES_API_REVISION`、
`RECORDING_COMPATIBILITY_REVISION` がいずれも `1` です。将来のrevisionは明示的に
選択する形式で追加し、既存の設定・Notebook・CSVを引き続き読み取ります。

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

## English

The GUI, acquisition, and device control run in Rust. This client passes commands and results over JSON; Python device drivers and Qt are not required.

### Compatibility

Profiles, `--stdio` JSON-lines commands/events, measurement CSVs, and adjacent `.run.json` files retain the unversioned revision-1 contract without adding fields to existing data. Arbitrary user metadata is not interpreted as a schema version. Python constants `PROFILE_COMPATIBILITY_REVISION`, `JSON_LINES_API_REVISION`, and `RECORDING_COMPATIBILITY_REVISION` are all `1`. Future revisions will be explicit opt-ins while existing profiles, notebooks, and CSVs remain readable.

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
