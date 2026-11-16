# KohdaLab TRKR 1.0.2

## Alignment and measurement / アライメントと測定

- Alignment groups the Signal and Power monitors above the Delay Stage and Scanner controls. Motion Only expands the selected motion controls. / Alignmentの上段にSignal・Power Monitor、下段にDelay StageとScannerを配置しました。Motion Onlyでは選択中の軸操作を拡大できます。
- Signal Monitor supports a fixed point count or continuous recording. Handed-off axes can move while hardware recording continues. A motion card's Stop cancels its active move. The monitor's Stop ends recording. / Signal Monitorで点数指定と連続記録を選べます。制御を引き渡した軸は実機記録中も動かせます。機器カードのStopは移動を止め、MonitorのStopは記録を終了します。
- Hover over measured points or map cells to see their coordinates and signal values. Finite runs estimate their remaining time from completed points and scan structure. / 測定点やマップの測定済みセルにカーソルを合わせると座標と信号値を表示します。有限の測定では完了点と走査構成から残り時間を見積もります。
- Windows position readouts fit within their controls. CONEX relative steps check the reported position against the controller target before issuing motion. / Windowsで位置表示のはみ出しを修正しました。CONEXの相対Stepは、実位置とコントローラーの目標を照合してから移動指令を送ります。

## Packages / 配布形式

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG | ZIP |
| Ubuntu 24.04 x64 | DEB | tar.gz |

Windows Setup installs under `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`. / Windowsのインストール先はProgram Files配下です。既定の設定ファイルはユーザーディレクトリに保存されます。
