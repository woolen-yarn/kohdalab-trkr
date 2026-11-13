# KohdaLab TRKR 1.0.0

**Candidate for lab review / レビュー用候補**

TRKR combines lock-in signal acquisition with delay-stage and spatial scanning. The Signal Monitor can use the lock-in independently. Lock-in settings are shown according to the connected model and read back from the instrument.
TRKRはロックイン信号取得と遅延・空間ステージ走査を行うアプリです。Signal Monitorではロックイン単体で測定できます。設定項目は接続機種に応じて表示し、設定値を実機から読み戻します。

See [hardware verification / 実機検証](validation/v1-hardware-results.md) for connection methods, tested operations, and remaining coverage. See the [control API / 外部制御API](reference/control-api.md) for programmatic operation.

## Packages / 配布形式

Ubuntu packages are intended for Ubuntu 24.04 x86_64. Hardware connections on Linux are unqualified.
Ubuntu版の対象は24.04 x86_64です。Linuxの実機接続は未検証です.

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG, drag to Applications / Applicationsへドラッグ | ZIP containing the app / アプリを含むZIP |
| Ubuntu x86_64 (build verification pending / ビルド検証待ち) | DEB | tar.gz |

The Windows installer uses `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`.
Windowsのインストール先は上記のProgram Files配下です。既定の設定ファイルはユーザーディレクトリ配下に保存されます。
