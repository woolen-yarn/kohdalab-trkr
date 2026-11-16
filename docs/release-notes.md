# KohdaLab TRKR 1.0.3

## Alignment and measurement / アライメントと測定

- Signal Monitor has a compact Start/Stop row, with Acquisition, Output and Device in Settings. The plot shows elapsed time from the start of recording and uses the same visual style as TRKR. / Signal MonitorのStart／Stopを一列にまとめ、SettingsにAcquisition・Output・Deviceを配置しました。グラフは記録開始からの経過時間をTRKRと同じ描画スタイルで表示します。
- TRKR, SRKR, STRKR and SRKR 2D estimate acquisition time from completed points and account for line transitions separately. Signal Monitor shows point counts. / TRKR・SRKR・STRKR・SRKR 2Dの残り取得時間を実測の完了点から計算し、行切り替えを別に扱います。Signal Monitorは点数を表示します。
- Signal and power plots use engineering prefixes based on the acquired values. Live history is separated when the signal unit changes. / 信号・パワーのプロットは取得値に応じてSI接頭語を選び、信号単位が変わるとライブ履歴を分けます。

Legacy replay data without an identifiable signal unit is displayed in its original numeric scale with the unit marked unknown. / 単位を特定できない旧CSVは、数値を換算せず、単位不明として再生します。

## Packages / 配布形式

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG | ZIP |
| Ubuntu 24.04 x64 | DEB | tar.gz |

Windows Setup installs under `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`. / Windowsのインストール先はProgram Files配下です。既定の設定ファイルはユーザーディレクトリに保存されます。
