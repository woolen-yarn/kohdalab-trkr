# Rust版をNotebookから使う

GUI・測定・装置制御はRustで動作します。このクライアントはJSONで命令と結果を渡す窓口です。Pythonの装置ドライバーやQtは不要です。

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

旧 `kohdalab.api.Experiment` と新クライアントは同じAPIではありません。既存の実験Notebookはコピーを作り、まず同梱の対応するNotebookでシミュレーションを確認してください。JSONはcanonicalな`instruments`／`measurements`形式が対象です。任意名の機器選択や旧Advanced形式など、未互換の設定も残っています。[互換性監査](https://github.com/woolen-yarn/kohdalab-trkr/blob/v0.3.2/docs/trkr_portable_rust.md)に残件を記載しています。測定CSVは専用Demoで読めますが、旧APIや保存形式全体の厳密な同一性を保証するものではありません。

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
