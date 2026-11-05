# KohdaLab TRKR 0.4.10

## Changes / 変更点

- Save Now copies CSV snapshots and their companion records in the background, keeping the GUI responsive. A pending copy is visible, and ordinary window closure/API takeover waits until it finishes. Complete-row and no-overwrite guarantees remain in place. / Save NowのCSV・付随記録のコピーを非同期化し、画面の応答を維持します。保存中を表示し、コピー完了まで通常終了・API引継ぎを防ぎます。完結した行の保存と上書き防止を維持します。
- Preserve whitespace in native file-picker results and show full Config/Output paths on hover without changing field widths. / ファイル選択結果の空白を保持し、入力欄の幅を変えずにConfig・Outputの全文をホバー表示します。
- Ignore empty home environment values before choosing a valid alternative. The common Python client evaluates its fallback only when needed and refuses a relative fallback. / 空のホーム環境変数を除外して有効な代替先を使用します。共通Pythonクライアントも必要時だけフォールバックを評価し、相対パスを拒否します。
- Avoid rescanning the entire accumulated API response for every network fragment. Existing size limits, timeouts and error reporting are retained. / API応答の受信断片ごとに全データを再走査する無駄を削減し、サイズ制限・タイムアウト・エラー処理を維持します。
- Source ZIPs mark the documented native rebuild entry points executable even when produced from a Windows checkout, while retaining ordinary source-file modes. / Windowsチェックアウトから作成した場合も、ソースZIP内のネイティブ再ビルド用エントリーポイントを実行可能として記録し、通常のソースファイルのモードは維持します。

## Compatibility / 互換性

Existing JSON profiles, CSV files and Notebook API calls remain compatible; no migration is needed. Measurement values, units, calibration, stage/scanner motion, lock-in acquisition and Stop protection are unchanged. Update the packaged client alongside the application when using Python.

既存JSON設定・CSV・Notebook APIは互換で、移行は不要です。測定値・単位・校正・ステージ／スキャナ移動・ロックイン取得・Stop保護は変更しません。Python利用時はアプリと同梱クライアントを一緒に更新してください。

## Validation and downloads / 検証・配布

Local software checks cover Rust, Python, packaging, async-copy completion/error and shared path resolution. Actual GUI simulation covers all five measurement tabs. Release CI checks Windows/macOS/Linux and builds Windows x64 Setup/portable ZIP and macOS Apple silicon DMG/ZIP, with corresponding source, licenses, provenance and SHA-256 manifests. Windows test fixtures use native absolute paths. These checks do not claim physical-instrument qualification.

ローカルではRust・Python・配布・非同期保存の完了／失敗・共通パス処理を検証し、実GUIの模擬データで5測定タブを確認します。公開CIはWindows・macOS・Linuxを検証し、Windows x64インストーラー／portable ZIP、Mac Apple silicon DMG／ZIPと、対応ソース・権利表記・来歴・SHA-256一覧を作成します。Windowsのテスト条件もOSに適した絶対パスへ修正しました。実機測定の認定を意味するものではありません。

Windows packages are unsigned. macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名で公証はありません。
