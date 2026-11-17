# KohdaLab TRKR 1.0.5

## Control and replay / 制御と再生

- Python stdio now provides bounded move completion through `move_and_wait()`. GUI control tolerates transient communication failure within the existing lease. / Python stdioの `move_and_wait()` で移動の終端を待てます。GUI制御は既存のリース期間内に一時的な通信障害から復帰できます。
- Alignment reports clean cancellation consistently and retains actual stop failures. Native GPIB disconnect requests graceful helper shutdown. / Alignmentの正常停止の扱いを揃え、停止失敗はエラーとして表示します。ネイティブGPIB切断時はヘルパーの正常終了を待ちます。
- Replay retains scan axes and row metadata, skips raw companion files when opening a folder, and uses the selected playback interval. / 再生で走査軸と行情報を保持します。フォルダ再生では付随するraw CSVを除外し、選んだ再生間隔を使います。
- Replay shows unavailable voltage data as missing. The app reports dropped display samples and reuses Alignment plot data between updates. / 電圧換算値のない記録を欠測として表示し、表示用の点が欠落した場合に通知します。Alignmentのプロット点列を更新間で再利用します。

## Packages / 配布形式

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG | ZIP |
| Ubuntu 24.04 x64 | DEB | tar.gz |

Windows Setup installs under `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`. / Windowsのインストール先はProgram Files配下です。既定の設定ファイルはユーザーディレクトリに保存されます。
