# 測定CSVリプレイGUI

`kohdalab-replay-gui` は、実測CSVを実機接続なしで繰り返し再生するデモ用GUIです。
測定結果の保存やタイムスタンプの書き換えは行いません。

画面には通常の `kohdalab-gui` と同じレイアウトを使用します。装置接続、移動、
設定変更は表示したまま、実機へ命令を送らないデモ動作になります。測定結果は
保存しません。通常GUIの `Save Now` はリプレイCSV用の `Load` に変わります。
Signal Monitorタブは無効です。

ウィンドウタイトルは `KohdaLab TRKR — DEMO MODE — NO HARDWARE` と表示され、
画面下部にも赤い `DEMO MODE・実機未接続` バッジを常時表示します。

## 対応測定

- TRKR
- SRKR
- STRKR
- SRKR 2D

## データの準備

再生したい測定のCSVを用意します。ファイル名と保存フォルダは自由です。
各測定タブで個別のCSVを選択できます。

CSVはKohdaLab TRKRが出力する統一スキーマを想定しています。すべての測定で
`X_V`、`Y_V`、`R_V`、`Theta_deg` が必要です。さらに、測定ごとに以下の
位置列を使用します。

| 測定 | 必要な位置 |
| --- | --- |
| TRKR | `t_cor_ps`、`target_t_cor_ps`、`t_ps` のいずれか |
| SRKR | XまたはYの corrected target/position |
| STRKR | `fast_axis` と `slow_axis` に対応する2軸 |
| SRKR 2D | XとYの2軸 |

## 起動

```powershell
uv run kohdalab-replay-gui
```

既定では `demo_csv` を使用します。ConfigをLoadすると、フォルダ内の
TRKR、SRKR、STRKR、SRKR 2D用CSVが各タブに自動選択されます。別フォルダを
使う場合は `--data-dir` で指定します。各測定タブの `Wait (s)` で、
1点ごとのデモ待機時間を設定します。

公開woolen-yarn版には実験CSVを同梱しません。`demo_csv/README.md` に従って
ローカルに配置してください。Kohdalab内部版へのデータ同梱手順は
`docs/demo_publishing_ja.md` に分離しています。

```powershell
uv run kohdalab-replay-gui --data-dir C:\path\to\demo-data
```

`Wait (s) = 0.0` は人工的な待機を行わない最速再生です。CSVを最大16点ずつ
まとめ、Plot更新を最大60 FPSに制限します。1フレームの描画完了を待ってから
次のバッチへ進むため、0秒でもGUIイベントは蓄積せず、`Stop` を操作できます。
必要な場合は、起動時のWait初期値を `--interval` でも指定できます。

```powershell
uv run kohdalab-replay-gui --data-dir C:\path\to\demo-data --interval 0
```

## 操作手順

1. Demo専用の `~/.kohdalab/config/default_demo.json` が選択されていることを確認し、
   `Load` を押します。通常版の `default.json` や最後に開いた設定とは分離されています。
2. `Connect All` を押します。設定済み装置が仮想接続され、実機通信は行いません。
3. TRKR、SRKR、STRKR、またはSRKR 2Dタブを選びます。
4. Replay CSV欄には `demo_csv` の該当ファイルが選択済みです。変更する場合だけ
   `Browse` を使用します。
5. Run欄の `Load` を押します。CSVの軸と範囲が測定条件欄へ反映されます。
6. `Start` で再生し、`Stop` で停止します。

## Windowsデスクトップから起動

`desktop\KohdaLab TRKR Demo.vbs` を対象にショートカットを作成すると、黒い
コンソールを表示せずDemo版を起動できます。PowerShellでrepo rootから実行します。

```powershell
$desktop = [Environment]::GetFolderPath("Desktop")
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $desktop "KohdaLab TRKR Demo.lnk"))
$shortcut.TargetPath = (Resolve-Path ".\desktop\KohdaLab TRKR Demo.vbs").Path
$shortcut.WorkingDirectory = (Resolve-Path ".").Path
$shortcut.Save()
```

起動に失敗した場合はrepo rootの `replay_gui_launcher.log` を確認してください。

再生中は、通常測定と同様に左パネルへ移動、待機、ロックイン読取、位置、
信号を表示します。中央と右パネルのスナップショット、ラインプロット、
2次元ヒートマップも更新します。CSVの末尾に達すると表示をクリアして
先頭から自動的に繰り返します。
