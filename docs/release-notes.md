# KohdaLab TRKR 1.0.0

[Published release / 公開版](https://github.com/woolen-yarn/kohdalab-trkr/releases/tag/v1.0.0)

TRKR combines lock-in signal acquisition with delay-stage and spatial scanning. The Signal Monitor can use the lock-in independently. Lock-in settings are shown according to the connected model and read back from the instrument.
TRKRはロックイン信号取得と遅延・空間ステージ走査を行うアプリです。Signal Monitorではロックイン単体で測定できます。設定項目は接続機種に応じて表示し、設定値を実機から読み戻します。

See [hardware verification / 実機検証](validation/v1-hardware-results.md) for connection methods, tested operations, and remaining coverage. See the [control API / 外部制御API](reference/control-api.md) for programmatic operation.

## Packages / 配布形式

Ubuntu x86_64 packaging is pending build verification.
Ubuntu x86_64版はビルド検証待ちです。

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG, drag to Applications / Applicationsへドラッグ | ZIP containing the app / アプリを含むZIP |

The Windows installer uses `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`.
Windowsのインストール先は上記のProgram Files配下です。既定の設定ファイルはユーザーディレクトリ配下に保存されます。
