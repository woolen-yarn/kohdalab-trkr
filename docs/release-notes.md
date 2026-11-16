# KohdaLab TRKR 1.0.4

## Alignment and connections / アライメントと接続

- All Connect starts a new connection attempt after Stop. / Stopを押した後もAll Connectで接続できるよう修正しました。
- Signal Monitor and Power Monitor share the same plot height and separate Start and Stop buttons. Signal channel selection is in Settings → Acquisition. Power Monitor shows a trend after Start and holds it after Stop. / 両モニターでプロットの高さを揃え、StartとStopを分けました。SignalのチャンネルはSettings → Acquisitionで選択します。PowerのプロットはStartで更新を始め、Stopで保持します。
- Signal Monitor saves acquired points to CSV and keeps the recording visible after Stop. / Signal MonitorはStopで取得済みの点をCSVに保存し、記録を表示したままにします。
- Opening Device on the lock-in card also expands Settings. / 左パネルのロックインでDeviceを開くとSettingsも展開します。

Power Monitor’s device choice applies to the current session. The trend shows up to the 180 most recent readings and stays visible after Stop. / Power Monitorの機器選択は現在のセッションに適用され、プロットには直近180点までを表示し、Stop後も保持します。

## Packages / 配布形式

| OS | Installer / インストーラー | Portable / ポータブル |
| --- | --- | --- |
| Windows x64 | Setup.exe | ZIP |
| macOS Apple Silicon | DMG | ZIP |
| Ubuntu 24.04 x64 | DEB | tar.gz |

Windows Setup installs under `C:\Program Files\Kohdalab\KohdaLab TRKR`. User settings default to `~/.kohdalab/trkr/config/config.json`. / Windowsのインストール先はProgram Files配下です。既定の設定ファイルはユーザーディレクトリに保存されます。
