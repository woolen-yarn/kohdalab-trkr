# KohdaLab TRKR 0.4.7

## Changes / 変更点

Both side panels now open at startup. In 0.4.6, the requested initial width (1512 logical pixels) was narrower than the expanded layout minimum (1566), so first-frame detection automatically collapsed both panels. This automatic collapse is removed, and normal, profile/API and Demo launch widths now use the expanded layout minimum. The existing buttons still collapse or expand either panel manually.

起動時に左右のパネルを表示します。0.4.6では初期幅1512論理ピクセルが展開時の最小幅1566を下回り、初回描画の判定で両側が自動収納されていました。自動収納を取り除き、通常・設定指定/API・Demoの起動幅を展開時の最小幅に揃えました。既存のボタンによる手動の収納・展開は維持します。

## Compatibility / 互換性

Existing JSON settings and measurement files remain compatible; no setting migration is required for this fix. Configuration stays at `~/.kohdalab/trkr/config/config.json` (Windows: `%USERPROFILE%\.kohdalab\trkr\config\config.json`). Device control, acquisition, CSV schemas and external-control/Stop policies are unchanged.

既存JSON設定・測定ファイルは引き続き利用でき、この修正に伴う設定の移行は不要です。設定場所はTRKR専用の `config/config.json` のままです。装置制御・測定・CSV形式・外部制御と停止条件は変更していません。

## Validation and downloads / 検証・配布

Desktop Rust tests and actual GUI simulation captures cover all five workflows, with both panels open and with either panel explicitly collapsed. Release CI runs Linux, macOS and Windows tests, packages Windows x64 Setup/portable ZIP and macOS Apple silicon DMG/ZIP, and verifies the Windows installer/uninstaller lifecycle. Corresponding source/build kits and SHA-256 manifests accompany the packages. No physical-device qualification is claimed for this UI fix.

デスクトップのRustテストと実GUIのシミュレーションで、全5測定画面の左右表示・片側収納を確認しました。リリースCIはLinux・macOS・Windowsでテストし、Windowsのインストーラー／portable ZIPとmacOS Apple siliconのDMG／ZIPを作成し、Windowsのインストール・削除も検証します。対応ソースとSHA-256一覧を同梱します。このUI修正では実機測定を行っていません。

Windows artifacts remain unsigned; macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名で公証はありません。
