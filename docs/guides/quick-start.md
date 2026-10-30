# KohdaLab TRKR quick start

1. Install to `C:\Program Files\Kohdalab\KohdaLab TRKR` on Windows or `/Applications/KohdaLab TRKR.app` on macOS, or extract the complete portable package.
2. Explore synthetic data using the separate Demo application before connecting hardware.
3. Load a reviewed profile, check the selected devices, ports, origins, calibration, and limits, then connect deliberately.
4. Choose a workflow and output location. Keep the CSV and its `.run.json` metadata together.

Python/Jupyter access: [client guide](../../bindings/python/README.md) · [notebooks](../../notebook/).

See [supported hardware](../reference/hardware.md) before physical operation.

## 日本語

1. Windowsでは`C:\Program Files\Kohdalab\KohdaLab TRKR`、macOSでは`/Applications/KohdaLab TRKR.app`へインストールするか、portable版をフォルダーごと展開します。
2. 実機接続前に、専用Demoアプリで合成データを試します。
3. 確認済み設定を読み込み、機器・ポート・原点・校正・制限を確認してから接続します。
4. 測定と保存先を選びます。CSVと`.run.json`は一緒に保管してください。

Python/Jupyterからの利用：[クライアント](../../bindings/python/README.md)・[Notebook](../../notebook/)。実機操作の前に[対応機器](../reference/hardware.md)を参照してください。

## Updating from 0.4.3 or older / 旧版からの更新

1. Stop measurements and disconnect devices. Keep a copy of your reviewed JSON; the old default location was `~/.kohdalab/config/trkr-next.json` (or `default.json`).
2. If the old Windows Setup installed into `%LOCALAPPDATA%\KohdaLab\TRKR`, uninstall that version first to remove its per-user shortcuts. Configuration files are retained. Install the new Setup as administrator into `C:\Program Files\Kohdalab\KohdaLab TRKR`. Portable users replace the complete extracted package instead.
3. The new automatic profile is `~/.kohdalab/trkr/config/config.json`. Existing JSON remains compatible, but old shared directories are **not loaded automatically**. Use Config **Browse → Load**, review ports/calibration/origins, and **Save** to the new dedicated path. Demo uses the separate `config/demo.json`.
4. Update the bundled Python client together with the GUI. Automatic Notebook discovery now uses the dedicated user profile; an explicit `profile=` or `TRKR_PROFILE` still works. The first normal launch creates `config.json` from built-in safe defaults; it does not connect hardware or enable outputs automatically.

1. 測定を停止して機器を切断し、確認済みJSONを控えてください。旧既定の保存先は`~/.kohdalab/config/trkr-next.json`（または`default.json`）でした。
2. 旧Windows Setupが`%LOCALAPPDATA%\KohdaLab\TRKR`へ入っている場合は先にアンインストールし、ユーザー単位の古いショートカットを除去してください。設定ファイルは保持されます。新Setupは管理者権限でProgram Filesへ導入します。portable版は展開フォルダー全体を更新します。
3. 新しい自動読込先は`~/.kohdalab/trkr/config/config.json`です。旧JSONの形式は互換ですが、共有ディレクトリの設定は**自動読込しません**。Configの**Browse → Load**で読み込み、ポート・校正・原点を確認して新しい専用パスへ**Save**してください。Demoは別の`config/demo.json`です。
4. GUIと同梱Pythonクライアントを一緒に更新してください。Notebookの自動検出も専用設定を使いますが、`profile=`・`TRKR_PROFILE`による明示指定は引き続き使えます。通常の初回起動では`config.json`を内蔵の安全な既定値から自動作成し、機器接続や出力有効化は行いません。
